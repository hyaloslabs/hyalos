//! S1.9: each event kind is accepted only from the origin allowed to send it, and log time does not go
//! backwards (ADR-0024).

use std::path::{Path, PathBuf};

use hyalos_engine::{Decider, Engine, EngineError, RuleViolation, Ruling};
use hyalos_log::{Log, LogKey};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_types::{
    Answer, Digest, Event, Gate, HoldReason, JobId, JobOutcome, Origin, ReasonCode, Resolution,
    Stakes, SubstrateName, TaskId, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A short reason code for a decision; these tests are about structure, not the code.
fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

fn key() -> LogKey {
    LogKey::from_seed([4; 32])
}

fn path(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("origin-time-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 * n)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

/// Submits `event` as the engine would: a hold is released through the call that records who decided.
fn accept(e: &mut Engine, n: u64, event: Event) -> Res {
    match event {
        Event::HoldResolved { hold, reason, .. } => {
            e.decide_hold(at(n), hold, Decider::Human, reason)?;
        }
        Event::TaskCancelled { task, reason, .. } => {
            e.cancel_task(at(n), task, Decider::Human, reason)?;
        }
        Event::HoldRefused { hold, reason } => {
            let held = e.state().holds()[&hold].clone();
            let request = GateRequest {
                gate: held.gate,
                inputs: held.inputs,
                stakes: held.stakes,
            };
            e.reevaluate_hold(at(n), hold, &request, Outcome::Refuse { reason })?;
        }
        Event::PolicyApproved {
            document, reason, ..
        } => {
            e.approve_policy(at(n), &document, Decider::Human, reason)?;
        }
        Event::HandleReleased {
            task,
            handle,
            reason,
            ..
        } => {
            e.release_handle(at(n), task, handle, Decider::Human, reason)?;
        }
        Event::AgentIssued {
            name,
            credential,
            reason,
            ..
        } => {
            e.issue_agent(at(n), name, credential, Decider::Human, reason)?;
        }
        Event::AgentRevoked { agent, reason, .. } => {
            e.revoke_agent(at(n), agent, Decider::Human, reason)?;
        }
        Event::MergeApproved {
            task,
            repo,
            pr,
            head,
            method,
            reason,
            ..
        } => {
            e.approve_merge(at(n), task, &repo, pr, head, method, Decider::Human, reason)?;
        }
        Event::PluginInstalled {
            name,
            digest,
            grant,
            source,
            reason,
            ..
        } => {
            e.install_plugin(at(n), name, digest, grant, source, Decider::Human, reason)?;
        }
        other => {
            e.submit(at(n), Origin::Engine, other)?;
        }
    }
    Ok(())
}

fn go(e: &mut Engine, n: u64, event: Event) -> Res<u64> {
    Ok(e.submit(at(n), Origin::Engine, event)?.seq)
}

/// What a kind needs in front of it to be legal, and the event itself.
fn ready(name: &str, kind: &str) -> Res<(Engine, Event)> {
    let mut e = Engine::open(&path(&format!("{name}-{kind}"))?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let event = match kind {
        "task.created" => Event::TaskCreated { request: d(1) },
        "task.completed" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            Event::TaskCompleted { task }
        }
        "job.started" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            Event::JobStarted {
                task,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            }
        }
        "job.ended" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            let job = JobId::from_seq(go(
                &mut e,
                2,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                    attestation: hyalos_types::Attestation::no_hardware_root(),
                },
            )?);
            Event::JobEnded {
                job,
                outcome: JobOutcome::Succeeded,
                reason: why(),
            }
        }
        "gate.answered" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            let job = JobId::from_seq(go(
                &mut e,
                2,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                    attestation: hyalos_types::Attestation::no_hardware_root(),
                },
            )?);
            Event::GateAnswered {
                job,
                gate: Gate::Egress,
                inputs: d(5),
                answer: Answer::Allow,
                reason: ReasonCode::unavailable(),
            }
        }
        "hold.resolved" | "task.cancelled" | "hold.refused" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            let job = JobId::from_seq(go(
                &mut e,
                2,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                    attestation: hyalos_types::Attestation::no_hardware_root(),
                },
            )?);
            let Ruling::Held(hold) = e.rule_gate(
                at(3),
                Origin::Broker,
                job,
                &GateRequest {
                    gate: Gate::Egress,
                    inputs: d(5),
                    stakes: Stakes::irreversible(),
                },
                Outcome::Hold {
                    reason: ReasonCode::timed_out(),
                    why: HoldReason::TimedOut,
                },
            )?
            else {
                return Err("expected a hold".into());
            };
            match kind {
                "hold.resolved" => Event::HoldResolved {
                    hold,
                    by: Resolution::Human,
                    reason: why(),
                    signed: hyalos_types::Signed::Unrecorded,
                },
                "task.cancelled" => Event::TaskCancelled {
                    task,
                    by: Resolution::Human,
                    reason: why(),
                    signed: hyalos_types::Signed::Unrecorded,
                },
                _ => Event::HoldRefused {
                    hold,
                    reason: why(),
                },
            }
        }
        "policy.approved" => Event::PolicyApproved {
            document: "permit (principal, action == Action::\"read\", resource);".into(),
            by: Resolution::Human,
            reason: why(),
            signed: hyalos_types::Signed::Unrecorded,
        },
        "handle.released" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            Event::HandleReleased {
                task,
                handle: d(9),
                by: Resolution::Human,
                reason: why(),
                signed: hyalos_types::Signed::Unrecorded,
            }
        }
        "agent.issued" => Event::AgentIssued {
            name: hyalos_types::AgentName::new("claude-code").ok_or("name")?,
            credential: d(9),
            by: Resolution::Human,
            reason: why(),
            signed: hyalos_types::Signed::Unrecorded,
        },
        "agent.revoked" => {
            let head = e.issue_agent(
                at(1),
                hyalos_types::AgentName::new("claude-code").ok_or("name")?,
                d(9),
                Decider::Human,
                why(),
            )?;
            Event::AgentRevoked {
                agent: hyalos_types::AgentId::from_seq(head.seq),
                by: Resolution::Human,
                reason: why(),
                signed: hyalos_types::Signed::Unrecorded,
            }
        }
        "merge.approved" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            Event::MergeApproved {
                task,
                repo: "octo/repo".into(),
                pr: 1,
                head: hyalos_types::CommitSha::new("0123456789abcdef0123456789abcdef01234567")
                    .ok_or("sha")?,
                method: hyalos_types::MergeMethod::Merge,
                by: Resolution::Human,
                reason: why(),
                signed: hyalos_types::Signed::Unrecorded,
            }
        }
        "plugin.installed" => Event::PluginInstalled {
            name: hyalos_types::PluginName::new("pdf").ok_or("name")?,
            digest: d(2),
            grant: hyalos_types::Grant::none(),
            source: hyalos_types::PluginSource::Local,
            by: Resolution::Human,
            reason: why(),
            signed: hyalos_types::Signed::Unrecorded,
        },
        "job.values" => {
            let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
            let job = JobId::from_seq(go(
                &mut e,
                2,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                    attestation: hyalos_types::Attestation::no_hardware_root(),
                },
            )?);
            Event::JobValues {
                job,
                values: vec![hyalos_types::TypedValue::Bool(true)],
            }
        }
        "archive.pruned" => {
            // Only an archive the log made can be named, so compact first and name where it ended.
            go(&mut e, 1, Event::TaskCreated { request: d(1) })?;
            let archive = path(&format!("{name}-{kind}-archive"))?.with_file_name("a.db");
            let done = e.compact(at(2), &archive)?;
            Event::ArchivePruned {
                last_seq: done.head.seq,
                last_hash: Digest::from_bytes(done.head.hash),
            }
        }
        other => return Err(format!("no scenario for {other}").into()),
    };
    Ok((e, event))
}

#[test]
fn the_broker_may_submit_nothing_but_a_gate_answer() -> Res {
    // Every kind but hold.opened has a scenario here; hold.opened is covered below, since it is only
    // legal inside the write that an unknown answer owes.
    for kind in Event::KINDS.into_iter().filter(|k| *k != "hold.opened") {
        let (mut e, event) = ready("broker", kind)?;
        assert_eq!(event.kind(), kind);
        let before = (e.head()?, e.state().clone());
        let result = e.submit(at(10), Origin::Broker, event.clone());

        if kind == "gate.answered" {
            assert!(
                result.is_ok(),
                "the broker may report a gate answer: {result:?}"
            );
            continue;
        }
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(RuleViolation::OriginNotAllowed {
                    kind: k,
                    origin: Origin::Broker
                })) if k == kind
            ),
            "{kind}: {result:?}"
        );
        assert_eq!(
            (e.head()?, e.state().clone()),
            before,
            "{kind}: nothing was written"
        );

        // The very same event is accepted from the engine, so the refusal was about the origin alone.
        accept(&mut e, 10, event)?;
    }
    Ok(())
}

#[test]
fn the_engine_may_submit_every_kind_including_a_gate_answer() -> Res {
    for kind in Event::KINDS.into_iter().filter(|k| *k != "hold.opened") {
        let (mut e, event) = ready("engine", kind)?;
        accept(&mut e, 10, event)?;
    }
    Ok(())
}

#[test]
fn the_origin_is_checked_before_the_state_so_a_refusal_reveals_nothing() -> Res {
    let mut e = Engine::open(&path("precedence")?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    // Task 99 does not exist, but the broker is told it may not do this, not that the task is unknown.
    let result = e.submit(
        at(1),
        Origin::Broker,
        Event::TaskCompleted {
            task: TaskId::from_seq(99),
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::Rule(RuleViolation::OriginNotAllowed { .. }))
    ));
    Ok(())
}

fn unknown_batch(job: JobId, task: TaskId, ended: Origin, opened: Origin) -> Vec<(Origin, Event)> {
    vec![
        (
            Origin::Broker,
            Event::GateAnswered {
                job,
                gate: Gate::Content,
                inputs: d(2),
                answer: Answer::Unknown,
                reason: ReasonCode::unavailable(),
            },
        ),
        (
            ended,
            Event::JobEnded {
                job,
                outcome: JobOutcome::Held,
                reason: why(),
            },
        ),
        (
            opened,
            Event::HoldOpened {
                task,
                gate: Gate::Content,
                inputs: d(2),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible(),
            },
        ),
    ]
}

#[test]
fn a_hold_and_the_job_ending_that_goes_with_it_cannot_come_from_the_broker() -> Res {
    let mut e = Engine::open(&path("hold-origin")?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
    let job = JobId::from_seq(go(
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

    for (ended, opened, refused) in [
        (Origin::Broker, Origin::Engine, "job.ended"),
        (Origin::Engine, Origin::Broker, "hold.opened"),
        (Origin::Broker, Origin::Broker, "job.ended"),
    ] {
        let result = e.submit_all(at(3), &unknown_batch(job, task, ended, opened));
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(RuleViolation::OriginNotAllowed { kind, origin: Origin::Broker })) if kind == refused
            ),
            "{refused}: {result:?}"
        );
        assert_eq!(
            (e.head()?, e.state().clone()),
            before,
            "nothing was written"
        );
    }

    // With the origins the ADR gives, the same batch is accepted.
    e.submit_all(
        at(3),
        &unknown_batch(job, task, Origin::Engine, Origin::Engine),
    )?;
    assert_eq!(e.state().holds().len(), 1);
    Ok(())
}

#[test]
fn a_gate_hold_is_recorded_with_the_gates_origin_and_the_engines_consequences() -> Res {
    let p = path("recorded-origins")?;
    let mut e =
        Engine::open(&p, key())?.with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let task = TaskId::from_seq(go(&mut e, 1, Event::TaskCreated { request: d(1) })?);
    let job = JobId::from_seq(go(
        &mut e,
        2,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    e.rule_gate(
        at(3),
        Origin::Broker,
        job,
        &GateRequest {
            gate: Gate::Content,
            inputs: d(2),
            stakes: Stakes::irreversible(),
        },
        Outcome::Hold {
            reason: ReasonCode::unavailable(),
            why: HoldReason::Unknown,
        },
    )?;
    drop(e);
    let rows: Vec<(String, Origin)> = Log::open(&p, key())?
        .events()?
        .into_iter()
        .map(|r| (r.kind, r.origin))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("task.created".to_string(), Origin::Engine),
            ("job.started".to_string(), Origin::Engine),
            ("gate.answered".to_string(), Origin::Broker),
            ("job.ended".to_string(), Origin::Engine),
            ("hold.opened".to_string(), Origin::Engine),
        ]
    );
    // And such a log reopens.
    Engine::open(&p, key())?.with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    Ok(())
}

// ---- time ------------------------------------------------------------------------------------------

#[test]
fn an_event_may_share_a_time_with_the_one_before_but_not_precede_it() -> Res {
    let mut e = Engine::open(&path("time")?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    go(&mut e, 5, Event::TaskCreated { request: d(1) })?;
    go(&mut e, 5, Event::TaskCreated { request: d(2) })?; // same time: fine
    let before = (e.head()?, e.state().clone());

    let result = e.submit(
        Timestamp::from_millis(4_999),
        Origin::Engine,
        Event::TaskCreated { request: d(3) },
    );
    assert!(
        matches!(
            result,
            Err(EngineError::Rule(RuleViolation::TimeWentBackwards { previous, at: attempted }))
                if previous == at(5) && attempted == Timestamp::from_millis(4_999)
        ),
        "{result:?}"
    );
    assert_eq!(
        (e.head()?, e.state().clone()),
        before,
        "nothing was written"
    );
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("4999") && message.contains("5000"),
        "{message}"
    );

    // A refusal does not move time: the next event at the old time is still fine, and so is a later one.
    go(&mut e, 5, Event::TaskCreated { request: d(4) })?;
    go(&mut e, 6, Event::TaskCreated { request: d(5) })?;
    assert_eq!(e.state().last_at(), Some(at(6)));
    Ok(())
}

#[test]
fn time_is_enforced_for_batches_and_gates_and_nothing_is_half_written() -> Res {
    let mut e = Engine::open(&path("time-batch")?, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let task = TaskId::from_seq(go(&mut e, 10, Event::TaskCreated { request: d(1) })?);
    let job = JobId::from_seq(go(
        &mut e,
        11,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    let before = (e.head()?, e.state().clone());

    let batch = unknown_batch(job, task, Origin::Engine, Origin::Engine);
    assert!(matches!(
        e.submit_all(at(2), &batch),
        Err(EngineError::Rule(RuleViolation::TimeWentBackwards { .. }))
    ));
    let gate = e.rule_gate(
        at(2),
        Origin::Broker,
        job,
        &GateRequest {
            gate: Gate::Content,
            inputs: d(2),
            stakes: Stakes::irreversible(),
        },
        Outcome::Hold {
            reason: ReasonCode::unavailable(),
            why: HoldReason::Unknown,
        },
    );
    assert!(matches!(
        gate,
        Err(EngineError::Rule(RuleViolation::TimeWentBackwards { .. }))
    ));
    assert_eq!((e.head()?, e.state().clone()), before);
    Ok(())
}

#[test]
fn time_survives_a_restart() -> Res {
    let p = path("time-restart")?;
    {
        let mut e = Engine::open(&p, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
        go(&mut e, 7, Event::TaskCreated { request: d(1) })?;
    }
    let mut e =
        Engine::open(&p, key())?.with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    assert_eq!(e.state().last_at(), Some(at(7)));
    assert!(matches!(
        e.submit(at(6), Origin::Engine, Event::TaskCreated { request: d(2) }),
        Err(EngineError::Rule(RuleViolation::TimeWentBackwards { .. }))
    ));
    Ok(())
}

// ---- replay: a validly signed log that breaks a rule is not loaded ---------------------------------

fn raw(p: &Path, rows: &[(u64, Origin, Event)]) -> Res {
    let mut log = Log::open(p, key())?;
    for (n, origin, event) in rows {
        log.append(at(*n), *origin, event.kind(), 1, &event.encode()?)?;
    }
    Ok(())
}

#[test]
fn a_signed_log_with_a_disallowed_origin_is_refused_on_open() -> Res {
    let p = path("replay-origin")?;
    raw(
        &p,
        &[
            (1, Origin::Engine, Event::TaskCreated { request: d(1) }),
            (
                2,
                Origin::Broker,
                Event::TaskCompleted {
                    task: TaskId::from_seq(1),
                },
            ),
        ],
    )?;
    match Engine::open(&p, key()) {
        Err(EngineError::Replay {
            seq: 2,
            violation:
                RuleViolation::OriginNotAllowed {
                    kind: "task.completed",
                    origin: Origin::Broker,
                },
        }) => Ok(()),
        other => Err(format!("expected a replay refusal at event 2, got {other:?}").into()),
    }
}

#[test]
fn a_signed_log_whose_time_goes_backwards_is_refused_on_open() -> Res {
    let p = path("replay-time")?;
    raw(
        &p,
        &[
            (9, Origin::Engine, Event::TaskCreated { request: d(1) }),
            (3, Origin::Engine, Event::TaskCreated { request: d(2) }),
        ],
    )?;
    match Engine::open(&p, key()) {
        Err(EngineError::Replay {
            seq: 2,
            violation: RuleViolation::TimeWentBackwards { .. },
        }) => Ok(()),
        other => Err(format!("expected a replay refusal at event 2, got {other:?}").into()),
    }
}

// ---- opening ---------------------------------------------------------------------------------------

#[test]
fn opening_verifies_the_file_that_was_opened_whether_or_not_it_was_there_before() -> Res {
    // New path: created, empty, verified trivially.
    let fresh = path("open-new")?;
    assert!(!fresh.exists());
    assert_eq!(
        Engine::open(&fresh, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]))
            .head()?,
        None
    );

    // An empty file somebody else made a moment ago is a new log too.
    let pre = path("open-empty-file")?;
    std::fs::write(&pre, b"")?;
    assert_eq!(
        Engine::open(&pre, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]))
            .head()?,
        None
    );

    // A tampered log that is already there is never loaded.
    let bad = path("open-tampered")?;
    {
        let mut e = Engine::open(&bad, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
        go(&mut e, 1, Event::TaskCreated { request: d(1) })?;
    }
    let mut bytes = std::fs::read(&bad)?;
    let needle = "01".repeat(32);
    let i = bytes
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
        .ok_or("digest not in the file")?;
    bytes[i] = b'f';
    std::fs::write(&bad, bytes)?;
    assert!(matches!(
        Engine::open(&bad, key()),
        Err(EngineError::Verify(_))
    ));
    Ok(())
}
