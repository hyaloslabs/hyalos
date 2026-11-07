//! S1.2: tasks, jobs and holds are projections of the event log (ADR-0003, ADR-0019).

use std::path::PathBuf;

use hyalos_engine::{
    Decider, Engine, EngineError, Reevaluation, Resolved, RuleViolation, State, TaskStatus, replay,
};
use hyalos_log::{Head, Log, LogError, LogKey};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_sim::Sim;
use hyalos_types::{
    Answer, Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, Origin, ReasonCode,
    Resolution, SeededRng, Sensitive, Stakes, SubstrateName, TaskId, Timestamp,
};

type Res = Result<(), Box<dyn std::error::Error>>;

/// A short reason code for a decision; these tests are about structure, not the code.
fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

fn key() -> LogKey {
    LogKey::from_seed([9; 32])
}

fn fresh_path(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("hyalos-engine-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 * n)
}

fn submit(e: &mut Engine, n: u64, event: Event) -> Result<u64, EngineError> {
    Ok(route(e, at(n), event)?.seq)
}

/// Submits an event, sending a hold's release through the API that records what justifies it. A
/// re-evaluation is given the allow that the hold's own gate would have to answer.
fn route(e: &mut Engine, at: Timestamp, event: Event) -> Result<Head, EngineError> {
    let Event::HoldResolved {
        hold, by, reason, ..
    } = event
    else {
        return e.submit(at, Origin::Engine, event);
    };
    let decider = match by {
        Resolution::Policy => Decider::Policy,
        Resolution::Verifier => Decider::Verifier,
        Resolution::Human => Decider::Human,
        Resolution::Reevaluated => {
            let held = e
                .state()
                .holds()
                .get(&hold)
                .ok_or(EngineError::Rule(RuleViolation::UnknownHold(hold)))?;
            let request = GateRequest {
                gate: held.gate,
                inputs: held.inputs,
                stakes: held.stakes,
            };
            return match e.reevaluate_hold(at, hold, &request, Outcome::Proceed { reason })? {
                Reevaluation::Released(head) => Ok(head),
                Reevaluation::StillHeld | Reevaluation::Refused(_) => Err(EngineError::Poisoned),
            };
        }
    };
    e.decide_hold(at, hold, decider, reason)
}

#[test]
fn a_task_runs_through_a_hold_to_completion() -> Res {
    let mut e = Engine::open(&fresh_path("lifecycle")?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    let task = TaskId::from_seq(submit(&mut e, 1, Event::TaskCreated { request: d(1) })?);
    assert_eq!(
        task.seq(),
        1,
        "an id is the sequence of the event that made it"
    );
    let job = JobId::from_seq(submit(
        &mut e,
        2,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    submit(
        &mut e,
        3,
        Event::JobEnded {
            job,
            outcome: JobOutcome::Held,
            reason: why(),
        },
    )?;
    let hold = HoldId::from_seq(submit(
        &mut e,
        4,
        Event::HoldOpened {
            task,
            gate: Gate::Egress,
            inputs: d(2),
            reason: HoldReason::Unknown,
            stakes: Stakes::irreversible(),
        },
    )?);
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Held);
    assert!(
        matches!(
            submit(
                &mut e,
                5,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                    attestation: hyalos_types::Attestation::no_hardware_root(),
                }
            ),
            Err(EngineError::Rule(RuleViolation::TaskNotOpen { .. }))
        ),
        "a held task starts no job"
    );
    submit(
        &mut e,
        6,
        Event::HoldResolved {
            hold,
            by: Resolution::Human,
            reason: why(),
            signed: hyalos_types::Signed::Unrecorded,
        },
    )?;
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Open);
    let job2 = JobId::from_seq(submit(
        &mut e,
        7,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    submit(
        &mut e,
        8,
        Event::JobEnded {
            job: job2,
            outcome: JobOutcome::Succeeded,
            reason: why(),
        },
    )?;
    submit(&mut e, 9, Event::TaskCompleted { task })?;

    let t = &e.state().tasks()[&task];
    assert_eq!(t.status(), TaskStatus::Completed);
    assert_eq!(t.completed_at, Some(at(9)));
    assert_eq!(
        e.state().holds()[&hold].resolved,
        Some(Resolved {
            at: at(6),
            by: Resolution::Human,
            reason: why()
        })
    );
    assert_eq!(
        e.state().jobs()[&job].ended,
        Some((at(3), JobOutcome::Held))
    );
    assert_eq!(
        e.state().last_seq(),
        8,
        "eight events were accepted; the refused one was not written"
    );
    assert!(e.state().invariant_violations().is_empty());
    Ok(())
}

#[test]
fn two_holds_keep_a_task_held_until_both_resolve() -> Res {
    let mut e = Engine::open(&fresh_path("two-holds")?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    let task = TaskId::from_seq(submit(&mut e, 1, Event::TaskCreated { request: d(1) })?);
    let open = |e: &mut Engine, n, gate| {
        submit(
            e,
            n,
            Event::HoldOpened {
                task,
                gate,
                inputs: d(3),
                reason: HoldReason::TimedOut,
                stakes: Stakes::routine(),
            },
        )
        .map(HoldId::from_seq)
    };
    let a = open(&mut e, 2, Gate::Egress)?;
    let b = open(&mut e, 3, Gate::Content)?;
    submit(
        &mut e,
        4,
        Event::HoldResolved {
            hold: a,
            by: Resolution::Policy,
            reason: why(),
            signed: hyalos_types::Signed::None,
        },
    )?;
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Held);
    submit(
        &mut e,
        5,
        Event::HoldResolved {
            hold: b,
            by: Resolution::Reevaluated,
            reason: why(),
            signed: hyalos_types::Signed::None,
        },
    )?;
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Open);
    Ok(())
}

#[test]
fn an_illegal_event_is_refused_and_nothing_is_written() -> Res {
    let path = fresh_path("refused")?;
    let mut e =
        Engine::open(&path, key())?.with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    let task = TaskId::from_seq(submit(&mut e, 1, Event::TaskCreated { request: d(1) })?);
    let job = JobId::from_seq(submit(
        &mut e,
        2,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    let before = (e.head()?, e.state().clone());

    let nobody = TaskId::from_seq(99);
    let cases: Vec<(Event, &str)> = vec![
        (
            Event::JobStarted {
                task: nobody,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
            "unknown task",
        ),
        (Event::TaskCompleted { task: nobody }, "unknown task"),
        (
            Event::JobEnded {
                job: JobId::from_seq(99),
                outcome: JobOutcome::Failed,
                reason: why(),
            },
            "unknown job",
        ),
        (
            Event::HoldResolved {
                hold: HoldId::from_seq(99),
                by: Resolution::Human,
                reason: why(),
                signed: hyalos_types::Signed::Unrecorded,
            },
            "unknown hold",
        ),
        (
            Event::TaskCompleted { task },
            "completing with a job running",
        ),
        (
            Event::HoldOpened {
                task,
                gate: Gate::Role,
                inputs: d(1),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible(),
            },
            "holding with a job running",
        ),
        (
            Event::JobStarted {
                task,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
            "a second job while one is running",
        ),
    ];
    for (event, what) in cases {
        let kind = event.kind();
        let result = e.submit(at(10), Origin::Engine, event);
        assert!(
            matches!(result, Err(EngineError::Rule(_))),
            "{what} ({kind}): {result:?}"
        );
        assert_eq!(
            (e.head()?, e.state().clone()),
            before,
            "{what} changed something"
        );
    }

    // The same applies to ending a job twice and resolving a hold twice.
    submit(
        &mut e,
        11,
        Event::JobEnded {
            job,
            outcome: JobOutcome::Succeeded,
            reason: why(),
        },
    )?;
    assert!(matches!(
        e.submit(
            at(12),
            Origin::Engine,
            Event::JobEnded {
                job,
                outcome: JobOutcome::Failed,
                reason: why(),
            }
        ),
        Err(EngineError::Rule(RuleViolation::JobNotRunning(_)))
    ));
    let hold = HoldId::from_seq(submit(
        &mut e,
        13,
        Event::HoldOpened {
            task,
            gate: Gate::Manifest,
            inputs: d(1),
            reason: HoldReason::Unknown,
            stakes: Stakes::routine(),
        },
    )?);
    submit(
        &mut e,
        14,
        Event::HoldResolved {
            hold,
            by: Resolution::Policy,
            reason: why(),
            signed: hyalos_types::Signed::None,
        },
    )?;
    assert!(matches!(
        route(
            &mut e,
            at(15),
            Event::HoldResolved {
                hold,
                by: Resolution::Human,
                reason: why(),
                signed: hyalos_types::Signed::Unrecorded,
            },
        ),
        Err(EngineError::Rule(RuleViolation::HoldNotOpen(_)))
    ));
    // A completed task takes no more holds, jobs or completions.
    submit(&mut e, 16, Event::TaskCompleted { task })?;
    for event in [
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
        Event::TaskCompleted { task },
        Event::HoldOpened {
            task,
            gate: Gate::Egress,
            inputs: d(1),
            reason: HoldReason::Unknown,
            stakes: Stakes::irreversible(),
        },
    ] {
        assert!(matches!(
            e.submit(at(17), Origin::Engine, event),
            Err(EngineError::Rule(_))
        ));
    }
    // Everything refused left the log exactly as long as the accepted events.
    assert_eq!(e.head()?.map(|h| h.seq), Some(e.state().last_seq()));
    drop(e);
    let log = Log::open(&path, key())?;
    assert_eq!(
        log.events()?.len() as u64,
        6,
        "only the six accepted events were written"
    );
    Ok(())
}

#[test]
fn deleting_the_projection_and_replaying_gives_identical_state() -> Res {
    // The acceptance criterion of S1.2. The projection lives only in memory, so deleting it is dropping
    // the engine; the log is all that remains.
    let path = fresh_path("rebuild")?;
    let live = {
        let mut e = Engine::open(&path, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
        let t1 = TaskId::from_seq(submit(&mut e, 1, Event::TaskCreated { request: d(1) })?);
        let t2 = TaskId::from_seq(submit(&mut e, 2, Event::TaskCreated { request: d(2) })?);
        let j = JobId::from_seq(submit(
            &mut e,
            3,
            Event::JobStarted {
                task: t1,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
        )?);
        submit(
            &mut e,
            4,
            Event::JobEnded {
                job: j,
                outcome: JobOutcome::DeadlineExceeded,
                reason: why(),
            },
        )?;
        submit(
            &mut e,
            5,
            Event::HoldOpened {
                task: t1,
                gate: Gate::Content,
                inputs: d(7),
                reason: HoldReason::TimedOut,
                stakes: Stakes::irreversible(),
            },
        )?;
        submit(&mut e, 6, Event::TaskCompleted { task: t2 })?;
        e.state().clone()
    };
    // Rebuilt two ways: by reopening the engine, and by folding the stored events directly.
    let reopened =
        Engine::open(&path, key())?.with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    assert_eq!(reopened.state(), &live);
    let folded: State = replay(&Log::open(&path, key())?.events()?)?;
    assert_eq!(folded, live);
    assert_eq!(live.holds().len(), 1);
    Ok(())
}

#[test]
fn a_log_that_fails_verification_is_not_loaded() -> Res {
    let path = fresh_path("tampered")?;
    {
        let mut e = Engine::open(&path, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
        submit(&mut e, 1, Event::TaskCreated { request: d(1) })?;
    }
    // Same file, wrong key: the chain fails at event 1.
    let err = Engine::open(&path, LogKey::from_seed([1; 32])).unwrap_err();
    assert!(matches!(err, EngineError::Log(LogError::WrongKey)), "{err}");
    // Edited payload: the trigger blocks UPDATE, so drop it the way an attacker with the file would.
    let conn = rusqlite::Connection::open(&path)?;
    conn.execute_batch("DROP TRIGGER IF EXISTS events_no_update;")?;
    let changed = conn.execute("UPDATE events SET payload = x'7b7d' WHERE seq = 1", [])?;
    drop(conn);
    if changed == 1 {
        assert!(matches!(
            Engine::open(&path, key()),
            Err(EngineError::Verify(_))
        ));
    }
    Ok(())
}

#[test]
fn a_validly_signed_log_with_an_illegal_event_is_refused_on_replay() -> Res {
    // The log layer stores any bytes. An engine bug, or a hostile writer holding the key, could sign an
    // event that makes no sense; replay must say which one rather than build wrong state.
    let path = fresh_path("illegal")?;
    {
        let mut log = Log::open(&path, key())?;
        let job_on_nothing = Event::JobStarted {
            task: TaskId::from_seq(42),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        };
        log.append(
            at(1),
            Origin::Engine,
            job_on_nothing.kind(),
            1,
            &job_on_nothing.encode()?,
        )?;
    }
    let err = Engine::open(&path, key()).unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::Replay {
                seq: 1,
                violation: RuleViolation::UnknownTask(_)
            }
        ),
        "{err}"
    );
    // And one it cannot read at all names the event too.
    let path = fresh_path("unreadable")?;
    {
        let mut log = Log::open(&path, key())?;
        log.append(
            at(1),
            Origin::Engine,
            "task.created",
            1,
            b"{\"kind\":\"task.created\",\"extra\":1}",
        )?;
    }
    assert!(matches!(
        Engine::open(&path, key()).unwrap_err(),
        EngineError::Event { seq: 1, .. }
    ));
    Ok(())
}

/// Any tier, with or without a sensitive category, so the random runs meet the whole authority matrix.
fn random_stakes(rng: &mut SeededRng) -> Stakes {
    let stakes = match rng.below(3) {
        0 => Stakes::routine(),
        1 => Stakes::escalated(),
        _ => Stakes::irreversible(),
    };
    match rng.below(6) {
        0 => stakes.touching(Sensitive::Credentials),
        1 => stakes.touching(Sensitive::Policy),
        2 => stakes.touching(Sensitive::Publishing),
        3 => stakes.touching(Sensitive::CapabilityGrant),
        _ => stakes,
    }
}

/// Picks a random event, legal or not, from what exists so far.
fn random_event(rng: &mut SeededRng, state: &State, seq_next: u64) -> Event {
    let pick = |rng: &mut SeededRng, n: usize| -> usize {
        usize::try_from(rng.below(n.max(1) as u64)).unwrap_or(0)
    };
    let task = |rng: &mut SeededRng| -> TaskId {
        let ids: Vec<TaskId> = state.tasks().keys().copied().collect();
        // Mostly real tasks, sometimes one that does not exist.
        if ids.is_empty() || rng.below(10) == 0 {
            TaskId::from_seq(seq_next + 1_000)
        } else {
            ids[pick(rng, ids.len())]
        }
    };
    let gate = [Gate::Egress, Gate::Content, Gate::Role, Gate::Manifest];
    let outcome = [
        JobOutcome::Succeeded,
        JobOutcome::Failed,
        JobOutcome::DeadlineExceeded,
        JobOutcome::Held,
    ];
    let by = [
        Resolution::Reevaluated,
        Resolution::Policy,
        Resolution::Verifier,
        Resolution::Human,
    ];
    match rng.below(7) {
        0 => Event::TaskCreated {
            request: Digest::from_bytes([rng.below(256) as u8; 32]),
        },
        1 => Event::TaskCompleted { task: task(rng) },
        2 => Event::JobStarted {
            task: task(rng),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
        3 => {
            let ids: Vec<JobId> = state.jobs().keys().copied().collect();
            let job = if ids.is_empty() || rng.below(10) == 0 {
                JobId::from_seq(seq_next + 1_000)
            } else {
                ids[pick(rng, ids.len())]
            };
            Event::JobEnded {
                job,
                outcome: outcome[pick(rng, outcome.len())],
                reason: why(),
            }
        }
        4 => Event::HoldOpened {
            task: task(rng),
            gate: gate[pick(rng, gate.len())],
            inputs: d(5),
            reason: if rng.below(2) == 0 {
                HoldReason::Unknown
            } else {
                HoldReason::TimedOut
            },
            stakes: random_stakes(rng),
        },
        5 => {
            // Allow and deny only: an unknown answer is not a single event (ADR-0020).
            let ids: Vec<JobId> = state.jobs().keys().copied().collect();
            let job = if ids.is_empty() || rng.below(10) == 0 {
                JobId::from_seq(seq_next + 1_000)
            } else {
                ids[pick(rng, ids.len())]
            };
            Event::GateAnswered {
                job,
                gate: gate[pick(rng, gate.len())],
                inputs: d(6),
                answer: if rng.below(2) == 0 {
                    Answer::Allow
                } else {
                    Answer::Deny
                },
                reason: ReasonCode::timed_out(),
            }
        }
        _ => {
            let ids: Vec<HoldId> = state.holds().keys().copied().collect();
            let hold = if ids.is_empty() || rng.below(10) == 0 {
                HoldId::from_seq(seq_next + 1_000)
            } else {
                ids[pick(rng, ids.len())]
            };
            Event::HoldResolved {
                hold,
                by: by[pick(rng, by.len())],
                reason: why(),
                signed: hyalos_types::Signed::None,
            }
        }
    }
}

#[test]
fn random_runs_keep_every_invariant_and_replay_to_the_same_state() -> Res {
    for seed in 0..8u64 {
        let sim = Sim::new(seed);
        let mut rng = sim.stream("events");
        let path = fresh_path(&format!("random-{seed}"))?;
        let mut e = Engine::open(&path, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
        let (mut accepted, mut refused) = (0u32, 0u32);
        for step in 0..200u64 {
            let next = e.state().last_seq() + 1;
            let event = random_event(&mut rng, e.state(), next);
            match route(&mut e, at(step), event) {
                Ok(_) => accepted += 1,
                Err(EngineError::Rule(_)) => refused += 1,
                Err(other) => return Err(format!("seed {seed} step {step}: {other}").into()),
            }
            let broken = e.state().invariant_violations();
            assert!(broken.is_empty(), "seed {seed} step {step}: {broken:?}");
        }
        assert!(
            accepted > 30 && refused > 30,
            "seed {seed}: {accepted} accepted, {refused} refused"
        );
        let live = e.state().clone();
        drop(e);
        assert_eq!(
            Engine::open(&path, key())?
                .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]))
                .state(),
            &live,
            "seed {seed}"
        );
        assert_eq!(
            live.last_seq(),
            u64::from(accepted),
            "refused events must not reach the log"
        );
    }
    Ok(())
}

#[test]
fn the_same_seed_gives_the_same_log_and_state() -> Res {
    let run = |name: &str| -> Result<(State, String), Box<dyn std::error::Error>> {
        let sim = Sim::new(77);
        let mut rng = sim.stream("events");
        let mut e = Engine::open(&fresh_path(name)?, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
        for step in 0..200u64 {
            let next = e.state().last_seq() + 1;
            let event = random_event(&mut rng, e.state(), next);
            let _ = e.submit(at(step), Origin::Engine, event);
        }
        let head = e.head()?.map(|h| h.to_string()).unwrap_or_default();
        Ok((e.state().clone(), head))
    };
    let (a, head_a) = run("det-a")?;
    let (b, head_b) = run("det-b")?;
    assert_eq!(a, b);
    assert_eq!(
        head_a, head_b,
        "identical events, times and key must give an identical chain head"
    );
    Ok(())
}
