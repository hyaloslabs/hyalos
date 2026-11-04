//! S1.6: every gate answers allow, deny or unknown, and unknown holds (ADR-0009, ADR-0020).

use std::path::{Path, PathBuf};
use std::time::Duration;

use hyalos_engine::{Decider, Engine, EngineError, RuleViolation, Ruling, TaskStatus, replay};
use hyalos_log::{Log, LogKey};
use hyalos_policy::{Evaluate, Fault, GateRequest, Outcome, Verdict, ask};
use hyalos_sim::{FakeClock, START, Sim};
use hyalos_types::{
    Answer, Clock, Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, Origin, ReasonCode,
    Stakes, SubstrateName, TaskId, Timestamp,
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
    let dir = std::env::temp_dir().join(format!("hyalos-gates-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn request() -> GateRequest {
    GateRequest {
        gate: Gate::Egress,
        inputs: d(8),
        stakes: Stakes::irreversible(),
    }
}

/// A gate with a fixed result that takes `takes` of fake time to give it.
struct Scripted {
    result: Result<Verdict, Fault>,
    takes: Duration,
    clock: FakeClock,
}

impl Evaluate for Scripted {
    fn evaluate(&mut self, _: &GateRequest) -> Result<Verdict, Fault> {
        self.clock.advance(self.takes);
        self.result.clone()
    }
}

fn says(answer: Answer, reason: &str) -> Result<Verdict, Fault> {
    Ok(Verdict {
        answer,
        reason: ReasonCode::new(reason).unwrap_or_else(ReasonCode::unavailable),
    })
}

/// An engine with one task and one running job, and a clock.
struct Rig {
    engine: Engine,
    clock: FakeClock,
    task: TaskId,
    job: JobId,
    path: PathBuf,
}

fn rig(name: &str) -> Result<Rig, Box<dyn std::error::Error>> {
    let path = fresh_path(name)?;
    let clock = FakeClock::new(START);
    let mut engine = Engine::open(&path, key())?;
    let task = TaskId::from_seq(
        engine
            .submit(
                clock.now(),
                Origin::Engine,
                Event::TaskCreated { request: d(1) },
            )?
            .seq,
    );
    let job = JobId::from_seq(
        engine
            .submit(
                clock.now(),
                Origin::Engine,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                },
            )?
            .seq,
    );
    Ok(Rig {
        engine,
        clock,
        task,
        job,
        path,
    })
}

impl Rig {
    /// Asks a scripted gate with a one second deadline and gives the outcome to the engine.
    fn gate(
        &mut self,
        result: Result<Verdict, Fault>,
        takes: Duration,
    ) -> Result<Ruling, EngineError> {
        let mut gate = Scripted {
            result,
            takes,
            clock: self.clock.clone(),
        };
        let deadline = Timestamp::from_millis(self.clock.now().as_millis() + 1_000);
        let outcome = ask(&mut gate, &self.clock, deadline, &request());
        self.engine.rule_gate(
            self.clock.now(),
            Origin::Broker,
            self.job,
            &request(),
            outcome,
        )
    }

    fn kinds(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let log = Log::open(&self.path, key())?;
        Ok(log.events()?.into_iter().map(|e| e.kind).collect())
    }
}

#[test]
fn a_gate_that_times_out_leaves_the_task_held_with_one_open_decision() -> Res {
    // The acceptance criterion of S1.6.
    let mut r = rig("timeout")?;
    let ruling = r.gate(
        says(Answer::Allow, "egress.host_allowed"),
        Duration::from_secs(5),
    )?;

    let Ruling::Held(hold) = ruling else {
        return Err(format!("expected a hold, got {ruling:?}").into());
    };
    let state = r.engine.state();
    let task = &state.tasks()[&r.task];
    assert_eq!(task.status(), TaskStatus::Held);
    assert_eq!(task.open_holds.len(), 1, "exactly one open decision");
    assert!(task.open_holds.contains(&hold));
    let h = &state.holds()[&hold];
    assert_eq!(
        (h.gate, h.inputs, h.reason),
        (Gate::Egress, d(8), HoldReason::TimedOut)
    );
    assert_eq!(h.resolved, None);
    // The job that asked is over, so nothing keeps running on a late "allow".
    assert_eq!(
        state.jobs()[&r.job].ended.map(|(_, o)| o),
        Some(JobOutcome::Held)
    );
    assert!(state.invariant_violations().is_empty());
    // The log tells it in order, and the answer on record is unknown, not the late allow.
    assert_eq!(
        r.kinds()?[2..],
        ["gate.answered", "job.ended", "hold.opened"]
    );
    let stored = Log::open(&r.path, key())?.events()?;
    let answer = Event::decode(
        &stored[2].kind,
        stored[2].schema_version,
        &stored[2].payload,
    )?;
    assert!(matches!(
        answer,
        Event::GateAnswered { answer: Answer::Unknown, ref reason, .. } if reason.as_str() == "gate.timed_out"
    ));
    Ok(())
}

#[test]
fn unknown_unavailable_and_reported_timeouts_hold_too() -> Res {
    let cases = [
        (
            says(Answer::Unknown, "egress.host_unlisted"),
            HoldReason::Unknown,
        ),
        (Err(Fault::Unavailable), HoldReason::Unknown),
        (Err(Fault::TimedOut), HoldReason::TimedOut),
    ];
    for (i, (result, why)) in cases.into_iter().enumerate() {
        let mut r = rig(&format!("holds-{i}"))?;
        let Ruling::Held(hold) = r.gate(result, Duration::ZERO)? else {
            return Err("expected a hold".into());
        };
        assert_eq!(r.engine.state().holds()[&hold].reason, why);
        assert_eq!(r.engine.state().tasks()[&r.task].open_holds.len(), 1);
    }
    Ok(())
}

#[test]
fn allow_proceeds_and_deny_refuses_and_neither_holds_or_ends_the_job() -> Res {
    let mut r = rig("allow-deny")?;
    assert_eq!(
        r.gate(says(Answer::Allow, "egress.host_allowed"), Duration::ZERO)?,
        Ruling::Proceed
    );
    assert_eq!(
        r.gate(says(Answer::Deny, "egress.host_denied"), Duration::ZERO)?,
        Ruling::Refused
    );
    let state = r.engine.state();
    assert!(state.jobs()[&r.job].is_running());
    assert_eq!(state.tasks()[&r.task].status(), TaskStatus::Open);
    assert!(state.holds().is_empty());
    assert_eq!(r.kinds()?[2..], ["gate.answered", "gate.answered"]);
    Ok(())
}

#[test]
fn a_held_task_is_released_by_resolving_its_one_hold() -> Res {
    let mut r = rig("release")?;
    let Ruling::Held(hold) = r.gate(Err(Fault::TimedOut), Duration::ZERO)? else {
        return Err("expected a hold".into());
    };
    let at = r.clock.now();
    assert!(matches!(
        r.engine.submit(
            at,
            Origin::Engine,
            Event::JobStarted {
                task: r.task,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded)
            }
        ),
        Err(EngineError::Rule(RuleViolation::TaskNotOpen { .. }))
    ));
    r.engine.decide_hold(at, hold, Decider::Human, why())?;
    assert_eq!(r.engine.state().tasks()[&r.task].status(), TaskStatus::Open);
    r.engine.submit(
        at,
        Origin::Engine,
        Event::JobStarted {
            task: r.task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
        },
    )?;
    Ok(())
}

#[test]
fn a_gate_for_a_job_that_is_not_running_is_refused() -> Res {
    let mut r = rig("not-running")?;
    let at = r.clock.now();
    r.engine.submit(
        at,
        Origin::Engine,
        Event::JobEnded {
            job: r.job,
            outcome: JobOutcome::Succeeded,
        },
    )?;
    let before = r.engine.head()?;
    for outcome in [
        Outcome::Proceed {
            reason: ReasonCode::timed_out(),
        },
        Outcome::Hold {
            reason: ReasonCode::timed_out(),
            why: HoldReason::TimedOut,
        },
    ] {
        let result = r
            .engine
            .rule_gate(at, Origin::Broker, r.job, &request(), outcome);
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(RuleViolation::JobNotRunning(_)))
            ),
            "{result:?}"
        );
    }
    let nobody = JobId::from_seq(999);
    let result = r.engine.rule_gate(
        at,
        Origin::Broker,
        nobody,
        &request(),
        Outcome::Proceed {
            reason: ReasonCode::timed_out(),
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::Rule(RuleViolation::UnknownJob(_)))
    ));
    assert_eq!(r.engine.head()?, before, "nothing was written");
    Ok(())
}

#[test]
fn an_unknown_answer_cannot_be_written_on_its_own() -> Res {
    let mut r = rig("lone-unknown")?;
    let before = (r.engine.head()?, r.engine.state().clone());
    let alone = Event::GateAnswered {
        job: r.job,
        gate: Gate::Egress,
        inputs: d(8),
        answer: Answer::Unknown,
        reason: ReasonCode::unavailable(),
    };
    let at = r.clock.now();
    assert!(matches!(
        r.engine.submit(at, Origin::Broker, alone.clone()),
        Err(EngineError::Rule(RuleViolation::UnknownNeedsHold))
    ));
    // Nor as a batch that stops short of the hold.
    assert!(matches!(
        r.engine.submit_all(at, &[(Origin::Broker, alone.clone())]),
        Err(EngineError::Rule(RuleViolation::UnknownNeedsHold))
    ));
    let ended = Event::JobEnded {
        job: r.job,
        outcome: JobOutcome::Held,
    };
    assert!(matches!(
        r.engine
            .submit_all(at, &[(Origin::Broker, alone), (Origin::Engine, ended)]),
        Err(EngineError::Rule(RuleViolation::UnknownNeedsHold))
    ));
    assert_eq!((r.engine.head()?, r.engine.state().clone()), before);
    Ok(())
}

#[test]
fn a_batch_that_breaks_a_rule_part_way_writes_nothing() -> Res {
    let mut r = rig("batch-atomic")?;
    let before = (r.engine.head()?, r.engine.state().clone());
    let at = r.clock.now();
    let batch = [
        (
            Origin::Broker,
            Event::GateAnswered {
                job: r.job,
                gate: Gate::Content,
                inputs: d(2),
                answer: Answer::Unknown,
                reason: ReasonCode::unavailable(),
            },
        ),
        (
            Origin::Engine,
            Event::JobEnded {
                job: r.job,
                outcome: JobOutcome::Held,
            },
        ),
        // The hold names different inputs from the answer, so it is not the hold that was owed.
        (
            Origin::Engine,
            Event::HoldOpened {
                task: r.task,
                gate: Gate::Content,
                inputs: d(3),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible(),
            },
        ),
    ];
    let result = r.engine.submit_all(at, &batch);
    assert!(
        matches!(
            result,
            Err(EngineError::Rule(RuleViolation::HoldRequired { .. }))
        ),
        "{result:?}"
    );
    assert_eq!((r.engine.head()?, r.engine.state().clone()), before);
    assert!(matches!(
        r.engine.submit_all(at, &[]),
        Err(EngineError::Log(_))
    ));
    Ok(())
}

/// Writes events straight into a log, the way a buggy engine or someone holding the key could.
fn write_raw(path: &Path, events: &[Event]) -> Result<(), Box<dyn std::error::Error>> {
    let mut log = Log::open(path, key())?;
    for e in events {
        log.append(START, Origin::Engine, e.kind(), 1, &e.encode()?)?;
    }
    Ok(())
}

fn unknown_answer(job: u64, inputs: Digest) -> Event {
    Event::GateAnswered {
        job: JobId::from_seq(job),
        gate: Gate::Egress,
        inputs,
        answer: Answer::Unknown,
        reason: ReasonCode::timed_out(),
    }
}

fn prefix() -> Vec<Event> {
    vec![
        Event::TaskCreated { request: d(1) },
        Event::JobStarted {
            task: TaskId::from_seq(1),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
        },
    ]
}

#[test]
fn replay_refuses_a_log_where_unknown_did_not_hold() -> Res {
    let task = TaskId::from_seq(1);
    let ended = Event::JobEnded {
        job: JobId::from_seq(2),
        outcome: JobOutcome::Held,
    };
    let hold = |inputs| Event::HoldOpened {
        task,
        gate: Gate::Egress,
        inputs,
        reason: HoldReason::TimedOut,
        stakes: Stakes::irreversible(),
    };
    let cases: Vec<(&str, Vec<Event>)> = vec![
        // Anything but the job's end right after an unknown answer.
        (
            "completed instead",
            vec![unknown_answer(2, d(8)), Event::TaskCompleted { task }],
        ),
        (
            "another gate answer",
            vec![unknown_answer(2, d(8)), unknown_answer(2, d(8))],
        ),
        (
            "job ended another way",
            vec![
                unknown_answer(2, d(8)),
                Event::JobEnded {
                    job: JobId::from_seq(2),
                    outcome: JobOutcome::Succeeded,
                },
            ],
        ),
        // The hold must come straight after the end, for the same gate and inputs.
        (
            "hold skipped",
            vec![
                unknown_answer(2, d(8)),
                ended.clone(),
                Event::TaskCompleted { task },
            ],
        ),
        (
            "hold for other inputs",
            vec![unknown_answer(2, d(8)), ended.clone(), hold(d(9))],
        ),
    ];
    for (name, tail) in cases {
        let path = fresh_path(&format!("raw-{}", name.replace(' ', "-")))?;
        let mut events = prefix();
        events.extend(tail);
        write_raw(&path, &events)?;
        let err = Engine::open(&path, key()).unwrap_err();
        assert!(
            matches!(
                err,
                EngineError::Replay {
                    violation: RuleViolation::HoldRequired { .. },
                    ..
                }
            ),
            "{name}: {err}"
        );
    }
    Ok(())
}

#[test]
fn replay_refuses_a_log_that_ends_before_the_hold_opened() -> Res {
    // A log cut short after the answer, or after the job ended, would otherwise open as a task that
    // looks free to run again.
    for (name, cut) in [("after-answer", 1), ("after-end", 2)] {
        let path = fresh_path(&format!("cut-{name}"))?;
        let mut events = prefix();
        events.push(unknown_answer(2, d(8)));
        events.push(Event::JobEnded {
            job: JobId::from_seq(2),
            outcome: JobOutcome::Held,
        });
        events.truncate(2 + cut);
        write_raw(&path, &events)?;
        let err = Engine::open(&path, key()).unwrap_err();
        assert!(
            matches!(
                err,
                EngineError::Replay {
                    violation: RuleViolation::UnfinishedHold(_),
                    ..
                }
            ),
            "{name}: {err}"
        );
    }
    Ok(())
}

#[test]
fn a_complete_hand_written_hold_replays() -> Res {
    let path = fresh_path("raw-good")?;
    let mut events = prefix();
    events.push(unknown_answer(2, d(8)));
    events.push(Event::JobEnded {
        job: JobId::from_seq(2),
        outcome: JobOutcome::Held,
    });
    events.push(Event::HoldOpened {
        task: TaskId::from_seq(1),
        gate: Gate::Egress,
        inputs: d(8),
        reason: HoldReason::TimedOut,
        stakes: Stakes::irreversible(),
    });
    write_raw(&path, &events)?;
    let e = Engine::open(&path, key())?;
    assert_eq!(
        e.state().tasks()[&TaskId::from_seq(1)].status(),
        TaskStatus::Held
    );
    assert_eq!(e.state().last_seq(), 5);
    assert_eq!(replay(&Log::open(&path, key())?.events()?)?, *e.state());
    Ok(())
}

#[test]
fn random_gate_outcomes_never_let_unknown_through() -> Res {
    for seed in 0..6u64 {
        let sim = Sim::new(seed);
        let mut rng = sim.stream("gates");
        let mut r = rig(&format!("random-{seed}"))?;
        let mut holds_made = 0u32;
        for step in 0..120 {
            // Start a job when the task is free, otherwise release it.
            let task = &r.engine.state().tasks()[&r.task];
            if r.engine.state().jobs()[&r.job].is_running() {
                let result = match rng.below(5) {
                    0 => says(Answer::Allow, "test.allow"),
                    1 => says(Answer::Deny, "test.deny"),
                    2 => says(Answer::Unknown, "test.unknown"),
                    3 => Err(Fault::Unavailable),
                    _ => Err(Fault::TimedOut),
                };
                let takes = Duration::from_millis(rng.below(2_000));
                let (allow, deny) = (
                    matches!(
                        result,
                        Ok(Verdict {
                            answer: Answer::Allow,
                            ..
                        })
                    ),
                    matches!(
                        result,
                        Ok(Verdict {
                            answer: Answer::Deny,
                            ..
                        })
                    ),
                );
                let open_before = task.open_holds.len();
                let ruling = r.gate(result, takes)?;
                let on_time = takes <= Duration::from_millis(1_000);
                assert_eq!(
                    matches!(ruling, Ruling::Proceed),
                    allow && on_time,
                    "seed {seed} step {step}"
                );
                assert_eq!(matches!(ruling, Ruling::Refused), deny && on_time);
                if let Ruling::Held(hold) = ruling {
                    holds_made += 1;
                    let t = &r.engine.state().tasks()[&r.task];
                    assert_eq!(t.open_holds.len(), open_before + 1, "one new open decision");
                    assert!(t.open_holds.contains(&hold));
                    assert_eq!(t.status(), TaskStatus::Held);
                    assert!(!r.engine.state().jobs()[&r.job].is_running());
                }
            } else {
                let at = r.clock.now();
                let open: Vec<HoldId> = task.open_holds.iter().copied().collect();
                for hold in open {
                    r.engine.decide_hold(at, hold, Decider::Human, why())?;
                }
                r.job = JobId::from_seq(
                    r.engine
                        .submit(
                            at,
                            Origin::Engine,
                            Event::JobStarted {
                                task: r.task,
                                substrate: SubstrateName::new("test")
                                    .unwrap_or_else(SubstrateName::unrecorded),
                            },
                        )?
                        .seq,
                );
            }
            let broken = r.engine.state().invariant_violations();
            assert!(
                broken.is_empty() && !r.engine.state().awaiting_hold(),
                "seed {seed} step {step}: {broken:?}"
            );
        }
        assert!(holds_made > 10, "seed {seed} made only {holds_made} holds");
        let live = r.engine.state().clone();
        let path = r.path.clone();
        drop(r);
        assert_eq!(Engine::open(&path, key())?.state(), &live, "seed {seed}");
    }
    Ok(())
}
