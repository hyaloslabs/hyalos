//! S1.7: the engine compacts its log into a signed snapshot and carries on from it, and a restore from
//! snapshot plus the remaining log gives the same state as the full log (ADR-0003, ADR-0026).

use std::path::PathBuf;

use hyalos_engine::{Decider, Engine, EngineError, RuleViolation, TaskStatus};
use hyalos_log::{CompactError, CopyError, Log, LogKey, Reason, VerifyError, verify_history};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_sim::Sim;
use hyalos_types::snapshot::{STATE_VERSION, StateImage};
use hyalos_types::{
    Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, Origin, ReasonCode, Resolution,
    Sensitive, Stakes, SubstrateName, TaskId, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([4; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("compaction-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

// ---- a workload both engines are driven through -----------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Create(u8),
    Start(TaskId),
    End(JobId, JobOutcome),
    Gate(JobId, u8, Stakes),
    Decide(HoldId),
    Complete(TaskId),
    /// End a held task with this decider.
    Cancel(TaskId, Decider),
    /// The gate refuses a held request again.
    Refuse(HoldId),
}

fn apply(e: &mut Engine, n: u64, op: &Op) -> Res {
    let t = at(n);
    match op {
        Op::Create(b) => {
            e.submit(t, Origin::Engine, Event::TaskCreated { request: d(*b) })?;
        }
        Op::Start(task) => {
            e.submit(
                t,
                Origin::Engine,
                Event::JobStarted {
                    task: *task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                },
            )?;
        }
        Op::End(job, outcome) => {
            e.submit(
                t,
                Origin::Engine,
                Event::JobEnded {
                    job: *job,
                    outcome: *outcome,
                    reason: why(),
                },
            )?;
        }
        Op::Gate(job, kind, stakes) => {
            let outcome = match kind {
                0 => Outcome::Proceed { reason: why() },
                1 => Outcome::Refuse { reason: why() },
                _ => Outcome::Hold {
                    reason: why(),
                    why: HoldReason::Unknown,
                },
            };
            let request = GateRequest {
                gate: Gate::Egress,
                inputs: d(*kind),
                stakes: *stakes,
            };
            e.rule_gate(t, Origin::Broker, *job, &request, outcome)?;
        }
        Op::Decide(hold) => {
            e.decide_hold(t, *hold, Decider::Human, why())?;
        }
        Op::Complete(task) => {
            e.submit(t, Origin::Engine, Event::TaskCompleted { task: *task })?;
        }
        Op::Cancel(task, decider) => {
            e.cancel_task(t, *task, *decider, why())?;
        }
        Op::Refuse(hold) => {
            let held = e.state().holds()[hold].clone();
            let request = GateRequest {
                gate: held.gate,
                inputs: held.inputs,
                stakes: held.stakes,
            };
            e.reevaluate_hold(t, *hold, &request, Outcome::Refuse { reason: why() })?;
        }
    }
    Ok(())
}

fn of<T: Copy>(v: &[T], pick: &mut dyn FnMut(u64) -> u64) -> T {
    v[pick(v.len() as u64) as usize]
}

/// A legal operation for the state `e` is in, chosen with `pick`.
fn choose(e: &Engine, pick: &mut dyn FnMut(u64) -> u64) -> Op {
    let state = e.state();
    let running: Vec<JobId> = state
        .jobs()
        .iter()
        .filter(|(_, j)| j.is_running())
        .map(|(id, _)| *id)
        .collect();
    let startable: Vec<TaskId> = state
        .tasks()
        .iter()
        .filter(|(_, t)| t.status() == TaskStatus::Open)
        .filter(|(id, _)| {
            !state
                .jobs()
                .values()
                .any(|j| j.task == **id && j.is_running())
        })
        .map(|(id, _)| *id)
        .collect();
    let open_holds: Vec<HoldId> = state
        .holds()
        .iter()
        .filter(|(id, _)| state.hold_is_open(id))
        .map(|(id, _)| *id)
        .collect();
    // Held tasks, each with a decider every one of its open holds allows (a human always qualifies).
    let cancellable: Vec<(TaskId, Vec<Decider>)> = state
        .tasks()
        .iter()
        .filter(|(_, t)| t.status() == TaskStatus::Held)
        .map(|(id, t)| {
            let ok = [Decider::Policy, Decider::Verifier, Decider::Human]
                .into_iter()
                .filter(|d| {
                    let by = match d {
                        Decider::Policy => Resolution::Policy,
                        Decider::Verifier => Resolution::Verifier,
                        Decider::Human => Resolution::Human,
                    };
                    t.open_holds
                        .iter()
                        .all(|h| state.holds()[h].stakes.may_be_cancelled_by(by))
                })
                .collect();
            (*id, ok)
        })
        .collect();

    let stakes = |pick: &mut dyn FnMut(u64) -> u64| {
        let s = match pick(3) {
            0 => Stakes::routine(),
            1 => Stakes::escalated(),
            _ => Stakes::irreversible(),
        };
        if pick(4) == 0 {
            s.touching(Sensitive::ALL[pick(4) as usize])
        } else {
            s
        }
    };
    match pick(12) {
        6..=7 if !cancellable.is_empty() => {
            let (task, deciders) = &cancellable[pick(cancellable.len() as u64) as usize];
            Op::Cancel(*task, deciders[pick(deciders.len() as u64) as usize])
        }
        8..=9 if !open_holds.is_empty() => Op::Refuse(of(&open_holds, pick)),
        1 if !startable.is_empty() => Op::Start(of(&startable, pick)),
        2 if !running.is_empty() => {
            let outcome = [
                JobOutcome::Succeeded,
                JobOutcome::Failed,
                JobOutcome::DeadlineExceeded,
            ][pick(3) as usize];
            Op::End(of(&running, pick), outcome)
        }
        3 if !running.is_empty() => Op::Gate(of(&running, pick), pick(3) as u8, stakes(pick)),
        4 if !open_holds.is_empty() => Op::Decide(of(&open_holds, pick)),
        5 if !startable.is_empty() => Op::Complete(of(&startable, pick)),
        _ => Op::Create(pick(250) as u8),
    }
}

// ---- 1. restore from snapshot plus the remaining log gives the state of the full log -------------------

#[test]
fn a_compacted_engine_and_one_that_never_compacted_agree_through_reopens_and_restarts() -> Res {
    let (mut compactions, mut reopens, mut cancelled, mut refusals) = (0, 0, 0, 0);
    for seed in 0..4u64 {
        let sim = Sim::new(seed);
        let mut rng = sim.stream("workload");
        let mut pick = |n: u64| rng.below(n);
        let tmp = dir(&format!("twin-{seed}"))?;
        let (live, twin) = (tmp.join("events.db"), tmp.join("twin.db"));
        let (mut a, mut b) = (Engine::open(&live, key())?, Engine::open(&twin, key())?);
        let mut archives: Vec<PathBuf> = Vec::new();

        for n in 0..110u64 {
            match pick(12) {
                0 if !a.state().tasks().is_empty() && a.head()?.is_some() => {
                    let archive = tmp.join(format!("archive-{seed}-{n}.db"));
                    match a.compact(at(n), &archive) {
                        Ok(c) => {
                            assert_eq!(Some(c.head), a.head()?);
                            archives.push(archive);
                            compactions += 1;
                        }
                        // Nothing came after the last snapshot.
                        Err(EngineError::Compact(CompactError::NothingToCompact)) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                1 => {
                    // A restart: the engine is rebuilt from the file alone.
                    drop(a);
                    a = Engine::open(&live, key())?;
                    reopens += 1;
                }
                _ => {
                    let op = choose(&a, &mut pick);
                    apply(&mut a, n, &op)?;
                    apply(&mut b, n, &op)?;
                }
            }
            assert_eq!(a.state(), b.state(), "seed {seed} step {n}");
            assert_eq!(
                a.head()?,
                b.head()?,
                "seed {seed} step {n}: the chain is unchanged"
            );
            let broken = a.state().invariant_violations();
            assert!(broken.is_empty(), "seed {seed} step {n}: {broken:?}");
        }

        // The full history: every archive, then the live log, from the first event.
        let history = verify_history(&live, &archives, &key().public(), a.head()?)?;
        assert!(history.from_genesis, "seed {seed}");
        // An independent replay of the archives agrees with every snapshot, cancellations included.
        let audited = hyalos_engine::audit(&live, &archives, &key().public(), None)?;
        assert!(audited.complete(), "seed {seed}: {audited:?}");
        cancelled += b
            .state()
            .tasks()
            .values()
            .filter(|t| t.cancelled.is_some())
            .count();
        refusals += Log::open(&twin, key())?
            .events()?
            .iter()
            .filter(|e| e.kind == "hold.refused")
            .count();
        // And the file alone, restarted, still means the same thing.
        drop(a);
        assert_eq!(
            Engine::open(&live, key())?.state(),
            b.state(),
            "seed {seed}"
        );
    }
    assert!(
        compactions >= 12 && reopens >= 12,
        "{compactions} compactions, {reopens} reopens"
    );
    assert!(
        cancelled >= 2 && refusals >= 2,
        "the workload must reach the new events: {cancelled} cancelled, {refusals} refusals"
    );
    Ok(())
}

// ---- 2. what a snapshot must preserve --------------------------------------------------------------------

/// Two tasks: one holding for a human, one with a job still running, plus a finished and a released one.
fn varied(e: &mut Engine) -> Res<(TaskId, TaskId, JobId)> {
    let mut n = 0;
    let mut step = |e: &mut Engine, op: Op| -> Res {
        n += 1;
        apply(e, n, &op)
    };
    step(e, Op::Create(1))?; // task 1
    step(e, Op::Start(TaskId::from_seq(1)))?; // job 2
    step(
        e,
        Op::Gate(
            JobId::from_seq(2),
            2,
            Stakes::irreversible().touching(Sensitive::Credentials),
        ),
    )?; // answer 3, end 4, hold 5
    step(e, Op::Create(2))?; // task 6
    step(e, Op::Start(TaskId::from_seq(6)))?; // job 7, left running
    step(e, Op::Create(3))?; // task 8
    step(e, Op::Start(TaskId::from_seq(8)))?; // job 9
    step(e, Op::End(JobId::from_seq(9), JobOutcome::Succeeded))?; // 10
    step(e, Op::Complete(TaskId::from_seq(8)))?; // 11
    step(e, Op::Create(4))?; // task 12
    step(e, Op::Start(TaskId::from_seq(12)))?; // job 13
    step(e, Op::Gate(JobId::from_seq(13), 2, Stakes::routine()))?; // answer 14, end 15, hold 16
    step(e, Op::Decide(HoldId::from_seq(16)))?; // 17: a released hold
    Ok((TaskId::from_seq(1), TaskId::from_seq(6), JobId::from_seq(7)))
}

#[test]
fn open_holds_running_jobs_and_released_holds_all_survive_a_compaction() -> Res {
    let tmp = dir("varied")?;
    let path = tmp.join("events.db");
    let mut e = Engine::open(&path, key())?;
    let (held, running_task, running_job) = varied(&mut e)?;
    let before = e.state().clone();
    assert_eq!(before.tasks()[&held].status(), TaskStatus::Held);

    let c = e.compact(at(100), &tmp.join("a1.db"))?;
    assert_eq!((c.head.seq, c.events), (17, 17));
    assert_eq!(
        e.state(),
        &before,
        "compacting does not touch the live state"
    );

    drop(e);
    let e = Engine::open(&path, key())?;
    let s = e.state();
    assert_eq!(s, &before);
    assert_eq!(s.tasks()[&held].status(), TaskStatus::Held);
    assert!(s.tasks()[&held].open_holds.contains(&HoldId::from_seq(5)));
    assert!(s.jobs()[&running_job].is_running());
    assert_eq!(s.jobs()[&running_job].task, running_task);
    let released = &s.holds()[&HoldId::from_seq(16)];
    assert!(released.resolved.is_some());
    let credentials = &s.holds()[&HoldId::from_seq(5)];
    assert_eq!(
        credentials.stakes,
        Stakes::irreversible().touching(Sensitive::Credentials),
        "stakes are kept, so authority still holds after a restore"
    );
    assert_eq!((s.last_seq(), s.last_at()), (17, Some(at(13))));
    Ok(())
}

#[test]
fn rules_keep_working_from_a_snapshot_with_no_events_after_it() -> Res {
    let tmp = dir("rules")?;
    let path = tmp.join("events.db");
    let mut e = Engine::open(&path, key())?;
    varied(&mut e)?;
    e.compact(at(100), &tmp.join("a1.db"))?;
    drop(e);
    let mut e = Engine::open(&path, key())?;
    assert!(
        Log::open(&path, key())?.events()?.is_empty(),
        "every event is in the archive; only the snapshot is left"
    );

    // Time does not go back across the snapshot.
    let early = e.submit(
        Timestamp::from_millis(1),
        Origin::Engine,
        Event::TaskCreated { request: d(9) },
    );
    assert!(
        matches!(
            early,
            Err(EngineError::Rule(RuleViolation::TimeWentBackwards { .. }))
        ),
        "{early:?}"
    );
    // Ids carry on from the snapshot: the next event is 18, and so is the task it makes.
    let head = e.submit(
        at(200),
        Origin::Engine,
        Event::TaskCreated { request: d(9) },
    )?;
    assert_eq!(head.seq, 18);
    assert!(e.state().tasks().contains_key(&TaskId::from_seq(18)));
    // A held task is still held, and the human can still release it.
    assert_eq!(
        e.state().tasks()[&TaskId::from_seq(1)].status(),
        TaskStatus::Held
    );
    e.decide_hold(at(201), HoldId::from_seq(5), Decider::Human, why())?;
    assert_eq!(
        e.state().tasks()[&TaskId::from_seq(1)].status(),
        TaskStatus::Open
    );
    Ok(())
}

#[test]
fn authority_over_a_hold_survives_a_restore_from_a_snapshot() -> Res {
    let tmp = dir("authority")?;
    let path = tmp.join("events.db");
    let mut e = Engine::open(&path, key())?;
    varied(&mut e)?;
    e.compact(at(100), &tmp.join("a1.db"))?;
    drop(e);
    let mut e = Engine::open(&path, key())?;
    for decider in [Decider::Policy, Decider::Verifier] {
        let result = e.decide_hold(at(200), HoldId::from_seq(5), decider, why());
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(
                    RuleViolation::ResolutionNotAuthorized { .. }
                ))
            ),
            "{decider:?}: {result:?}"
        );
    }
    Ok(())
}

// ---- 3. when compaction must refuse ------------------------------------------------------------------

#[test]
fn compaction_refuses_to_go_back_in_time_to_overwrite_or_to_do_nothing() -> Res {
    let tmp = dir("refuse")?;
    let path = tmp.join("events.db");
    let mut e = Engine::open(&path, key())?;
    let archive = tmp.join("a1.db");
    assert!(matches!(
        e.compact(at(1), &archive),
        Err(EngineError::Compact(CompactError::NothingToCompact))
    ));
    varied(&mut e)?;
    let before = e.state().clone();

    // Earlier than the last event.
    assert!(matches!(
        e.compact(Timestamp::from_millis(1), &archive),
        Err(EngineError::Rule(RuleViolation::TimeWentBackwards { .. }))
    ));
    assert!(!archive.exists());
    // An archive that is already there is never overwritten.
    std::fs::write(&archive, b"precious")?;
    assert!(matches!(
        e.compact(at(100), &archive),
        Err(EngineError::Compact(CompactError::Archive(
            CopyError::Exists(_)
        )))
    ));
    assert_eq!(std::fs::read(&archive)?, b"precious");
    assert_eq!(e.state(), &before);
    assert!(
        Log::open(&path, key())?.latest_anchor()?.is_none(),
        "nothing was compacted"
    );

    // And a second compaction with nothing new does nothing.
    e.compact(at(100), &tmp.join("a2.db"))?;
    assert!(matches!(
        e.compact(at(101), &tmp.join("a3.db")),
        Err(EngineError::Compact(CompactError::NothingToCompact))
    ));
    assert!(!tmp.join("a3.db").exists());
    Ok(())
}

// ---- 4. a snapshot this build cannot use is refused, never guessed at -------------------------------------

/// A signed snapshot whose state bytes are whatever the test wants, on top of a real log.
fn log_with_snapshot(
    name: &str,
    version: u32,
    state: impl FnOnce(&StateImage) -> Vec<u8>,
) -> Res<PathBuf> {
    let tmp = dir(name)?;
    let path = tmp.join("events.db");
    let mut e = Engine::open(&path, key())?;
    varied(&mut e)?;
    let image = e
        .state()
        .to_image()
        .map_err(|j| format!("unfinished {j}"))?;
    drop(e);
    let mut log = Log::open(&path, key())?;
    let archived = log.archive(&tmp.join("a.db"))?;
    log.compact::<String>(archived, at(100), |_, _| Ok((version, state(&image))))?;
    Ok(path)
}

/// `(name, state version, how to write the state, what the refusal must say)`.
type Case = (
    &'static str,
    u32,
    Box<dyn Fn(&StateImage) -> Vec<u8>>,
    &'static str,
);

#[test]
fn a_snapshot_that_is_not_a_state_this_build_can_read_stops_the_engine() -> Res {
    let good = |i: &StateImage| i.encode().unwrap_or_default();
    // The control: the same route with a good state opens.
    Engine::open(&log_with_snapshot("good", STATE_VERSION, good)?, key())?;

    let cases: Vec<Case> = vec![
        (
            "newer version",
            STATE_VERSION + 1,
            Box::new(good),
            "Upgrade",
        ),
        ("version zero", 0, Box::new(good), "version 0"),
        (
            "garbage",
            STATE_VERSION,
            Box::new(|_| b"not json".to_vec()),
            "cannot read",
        ),
        (
            "wrong event",
            STATE_VERSION,
            Box::new(|i| {
                let mut i = i.clone();
                i.last_seq += 1;
                i.encode().unwrap_or_default()
            }),
            "ends at event",
        ),
        (
            "duplicate task",
            STATE_VERSION,
            Box::new(|i| {
                let mut i = i.clone();
                i.tasks.push(i.tasks[0].clone());
                i.encode().unwrap_or_default()
            }),
            "appears twice",
        ),
        (
            "breaks the rules",
            STATE_VERSION,
            Box::new(|i| {
                let mut i = i.clone();
                // Release the hold in the hold table but leave the task listing it as open.
                for h in &mut i.holds {
                    if h.resolved.is_none() {
                        h.resolved = Some(hyalos_types::snapshot::ResolvedImage {
                            at: 1,
                            by: hyalos_types::Resolution::Human,
                            reason: ReasonCode::unavailable(),
                        });
                    }
                }
                i.encode().unwrap_or_default()
            }),
            "wrong open holds",
        ),
    ];
    for (name, version, state, expect) in cases {
        let path = log_with_snapshot(&format!("bad-{}", name.replace(' ', "-")), version, state)?;
        match Engine::open(&path, key()) {
            Err(EngineError::Snapshot(message)) => {
                assert!(message.contains(expect), "{name}: {message}");
            }
            other => return Err(format!("{name}: expected a refusal, got {other:?}").into()),
        }
    }
    Ok(())
}

#[test]
fn a_snapshot_edited_without_the_key_stops_the_engine_before_it_is_read() -> Res {
    let path = log_with_snapshot("tampered", STATE_VERSION, |i| {
        i.encode().unwrap_or_default()
    })?;
    rusqlite::Connection::open(&path)?
        .execute_batch("DROP TRIGGER anchors_no_update; UPDATE anchors SET state = x'7b7d';")?;
    assert!(matches!(
        Engine::open(&path, key()),
        Err(EngineError::Verify(VerifyError::Anchor {
            reason: Reason::StateDigestMismatch,
            ..
        }))
    ));
    Ok(())
}

// ---- 5. backup ------------------------------------------------------------------------------------------

#[test]
fn a_backup_of_a_compacted_log_restores_to_the_same_state() -> Res {
    let tmp = dir("backup")?;
    let path = tmp.join("events.db");
    let mut e = Engine::open(&path, key())?;
    varied(&mut e)?;
    e.compact(at(100), &tmp.join("a1.db"))?;
    apply(&mut e, 120, &Op::Create(77))?;
    let state = e.state().clone();
    let head = e.head()?;
    drop(e);

    let backup = tmp.join("backup.db");
    hyalos_log::copy_verified(&path, &backup, &key().public(), head)?;
    assert_eq!(Engine::open(&backup, key())?.state(), &state);
    Ok(())
}
