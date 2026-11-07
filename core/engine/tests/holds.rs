//! S1.10: a hold is released only by a fresh on-time allow or by a decision at the tier of the action,
//! each recorded with who decided (ADR-0009, ADR-0012, ADR-0025).

use std::path::PathBuf;
use std::time::Duration;

use hyalos_engine::{
    Decider, Engine, EngineError, Reevaluation, Resolved, RuleViolation, Ruling, TaskStatus,
};
use hyalos_log::Log;
use hyalos_log::LogKey;
use hyalos_policy::{Evaluate, Fault, GateRequest, Outcome, Verdict, ask};
use hyalos_sim::{FakeClock, START, Sim};
use hyalos_types::event::SCHEMA_VERSION;
use hyalos_types::{
    Answer, Clock, Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, Origin, ReasonCode,
    Resolution, Sensitive, Stakes, SubstrateName, TaskId, Tier, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([6; 32])
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn code(text: &str) -> Res<ReasonCode> {
    ReasonCode::new(text).ok_or_else(|| format!("bad reason code {text}").into())
}

fn path(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("holds-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
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

fn says(answer: Answer) -> Result<Verdict, Fault> {
    Ok(Verdict {
        answer,
        reason: ReasonCode::unavailable(),
    })
}

struct World {
    engine: Engine,
    clock: FakeClock,
    path: PathBuf,
    next: u8,
}

fn world(name: &str) -> Res<World> {
    let path = path(name)?;
    Ok(World {
        engine: Engine::open(&path, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32])),
        clock: FakeClock::new(START),
        path,
        next: 1,
    })
}

struct Held {
    task: TaskId,
    job: JobId,
    hold: HoldId,
    request: GateRequest,
}

impl World {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// A new task whose job asks `gate` something with these stakes, and the gate answers unknown.
    fn held(&mut self, stakes: Stakes) -> Res<Held> {
        self.next += 1;
        let request = GateRequest {
            gate: Gate::Egress,
            inputs: d(self.next),
            stakes,
        };
        let task = TaskId::from_seq(
            self.engine
                .submit(
                    self.now(),
                    Origin::Engine,
                    Event::TaskCreated { request: d(1) },
                )?
                .seq,
        );
        let job = JobId::from_seq(
            self.engine
                .submit(
                    self.now(),
                    Origin::Engine,
                    Event::JobStarted {
                        task,
                        substrate: SubstrateName::new("test")
                            .unwrap_or_else(SubstrateName::unrecorded),
                        credentials: hyalos_types::CredentialTier::BrokerHeld,
                        attestation: hyalos_types::Attestation::no_hardware_root(),
                    },
                )?
                .seq,
        );
        let Ruling::Held(hold) = self.engine.rule_gate(
            self.now(),
            Origin::Broker,
            job,
            &request,
            Outcome::Hold {
                reason: ReasonCode::unavailable(),
                why: HoldReason::Unknown,
            },
        )?
        else {
            return Err("expected a hold".into());
        };
        Ok(Held {
            task,
            job,
            hold,
            request,
        })
    }

    /// Asks the held gate again with a scripted result and a one second deadline.
    fn ask_again(
        &mut self,
        held: &Held,
        result: Result<Verdict, Fault>,
        takes: Duration,
    ) -> Result<Reevaluation, EngineError> {
        let mut gate = Scripted {
            result,
            takes,
            clock: self.clock.clone(),
        };
        let deadline = Timestamp::from_millis(self.now().as_millis() + 1_000);
        let outcome = ask(&mut gate, &self.clock, deadline, &held.request);
        self.engine
            .reevaluate_hold(self.now(), held.hold, &held.request, outcome)
    }

    fn snapshot(&self) -> Res<(Option<hyalos_log::Head>, hyalos_engine::State)> {
        Ok((self.engine.head()?, self.engine.state().clone()))
    }

    fn resolved(&self, hold: HoldId) -> Option<Resolved> {
        self.engine.state().holds()[&hold].resolved.clone()
    }
}

const QUICK: Duration = Duration::from_millis(10);

// ---- 1. re-evaluation ------------------------------------------------------------------------------

#[test]
fn a_fresh_on_time_allow_releases_the_hold_as_reevaluated() -> Res {
    let mut w = world("reeval-allow")?;
    let h = w.held(Stakes::routine())?;
    assert_eq!(w.engine.state().tasks()[&h.task].status(), TaskStatus::Held);

    let Reevaluation::Released(head) = w.ask_again(&h, says(Answer::Allow), QUICK)? else {
        return Err("an on-time allow should release".into());
    };
    let resolved = w.resolved(h.hold).ok_or("not resolved")?;
    assert_eq!(resolved.by, Resolution::Reevaluated);
    assert_eq!(resolved.reason, ReasonCode::unavailable());
    assert_eq!(Some(resolved.at), w.engine.state().last_at());
    assert_eq!(w.engine.state().last_seq(), head.seq);
    assert_eq!(w.engine.state().tasks()[&h.task].status(), TaskStatus::Open);
    // The held job stays ended; a new job is what runs next.
    let again = w
        .engine
        .submit(
            w.now(),
            Origin::Engine,
            Event::JobStarted {
                task: h.task,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
        )?
        .seq;
    assert_ne!(again, h.job.seq());

    // Recorded as an engine event, with the decider and reason in it.
    let stored = Log::open(&w.path, key())?.events()?;
    let last = stored
        .iter()
        .find(|e| e.seq == head.seq)
        .ok_or("no such event")?;
    assert_eq!(
        (last.kind.as_str(), last.origin),
        ("hold.resolved", Origin::Engine)
    );
    let text = String::from_utf8(last.payload.clone())?;
    assert!(
        text.contains(r#""by":"reevaluated""#) && text.contains(r#""reason":"#),
        "{text}"
    );
    Ok(())
}

#[test]
fn unknown_a_timeout_and_a_late_allow_leave_the_hold_open_and_write_nothing() -> Res {
    let mut w = world("reeval-not-allow")?;
    let h = w.held(Stakes::routine())?;
    let cases: Vec<(&str, Result<Verdict, Fault>, Duration)> = vec![
        ("unknown", says(Answer::Unknown), QUICK),
        ("gate timed out", Err(Fault::TimedOut), QUICK),
        ("gate unavailable", Err(Fault::Unavailable), QUICK),
        // Says allow, but after the deadline: the caller has already been told it did not answer.
        ("late allow", says(Answer::Allow), Duration::from_secs(5)),
    ];
    for (name, result, takes) in cases {
        let before = w.snapshot()?;
        for _ in 0..3 {
            assert_eq!(
                w.ask_again(&h, result.clone(), takes)?,
                Reevaluation::StillHeld,
                "{name}"
            );
        }
        assert_eq!(w.snapshot()?, before, "{name}: nothing may be written");
        assert!(w.resolved(h.hold).is_none(), "{name}");
        assert_eq!(
            w.engine.state().tasks()[&h.task].status(),
            TaskStatus::Held,
            "{name}"
        );
    }
    // And the hold can still be released by a real allow afterwards.
    assert!(matches!(
        w.ask_again(&h, says(Answer::Allow), QUICK)?,
        Reevaluation::Released(_)
    ));
    Ok(())
}

#[test]
fn an_allow_for_another_gate_or_other_inputs_does_not_release_the_hold() -> Res {
    let mut w = world("reeval-other")?;
    let h = w.held(Stakes::routine())?;
    let before = w.snapshot()?;
    let allow = || Outcome::Proceed {
        reason: ReasonCode::unavailable(),
    };
    let other_inputs = GateRequest {
        inputs: d(200),
        ..h.request
    };
    let other_gate = GateRequest {
        gate: Gate::Content,
        ..h.request
    };
    for request in [other_inputs, other_gate] {
        let result = w.engine.reevaluate_hold(w.now(), h.hold, &request, allow());
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(RuleViolation::ReevaluatedAnotherRequest(x))) if x == h.hold
            ),
            "{result:?}"
        );
    }
    assert_eq!(w.snapshot()?, before);
    Ok(())
}

#[test]
fn only_an_open_hold_can_be_asked_again() -> Res {
    let mut w = world("reeval-missing")?;
    let h = w.held(Stakes::routine())?;
    let ghost = HoldId::from_seq(999);
    assert!(matches!(
        w.engine.reevaluate_hold(
            w.now(),
            ghost,
            &h.request,
            Outcome::Proceed {
                reason: ReasonCode::unavailable()
            }
        ),
        Err(EngineError::Rule(RuleViolation::UnknownHold(_)))
    ));
    assert!(matches!(
        w.ask_again(&h, says(Answer::Allow), QUICK)?,
        Reevaluation::Released(_)
    ));
    // Once released, asking again is an error, not a second release.
    assert!(matches!(
        w.ask_again(&h, says(Answer::Allow), QUICK),
        Err(EngineError::Rule(RuleViolation::HoldNotOpen(_)))
    ));
    Ok(())
}

#[test]
fn a_gate_allow_cannot_release_a_hold_that_needs_a_decision() -> Res {
    for (name, stakes) in [
        ("irreversible", Stakes::irreversible()),
        (
            "credentials",
            Stakes::routine().touching(Sensitive::Credentials),
        ),
        (
            "escalated publishing",
            Stakes::escalated().touching(Sensitive::Publishing),
        ),
    ] {
        let mut w = world(&format!("reeval-barred-{}", name.replace(' ', "-")))?;
        let h = w.held(stakes)?;
        let before = w.snapshot()?;
        let result = w.ask_again(&h, says(Answer::Allow), QUICK);
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(RuleViolation::ResolutionNotAuthorized {
                    by: Resolution::Reevaluated,
                    ..
                }))
            ),
            "{name}: {result:?}"
        );
        assert_eq!(w.snapshot()?, before, "{name}");
    }
    Ok(())
}

// ---- 2. the only ways to release a hold -------------------------------------------------------------

#[test]
fn a_hold_is_released_only_through_the_calls_that_record_what_justifies_it() -> Res {
    let mut w = world("api-only")?;
    let h = w.held(Stakes::routine())?;
    let before = w.snapshot()?;
    let release = || Event::HoldResolved {
        hold: h.hold,
        by: Resolution::Human,
        reason: ReasonCode::unavailable(),
        signed: hyalos_types::Signed::Unrecorded,
    };
    assert!(matches!(
        w.engine.submit(w.now(), Origin::Engine, release()),
        Err(EngineError::Rule(RuleViolation::ResolveThroughTheEngineApi))
    ));
    assert!(matches!(
        w.engine.submit_all(w.now(), &[(Origin::Engine, release())]),
        Err(EngineError::Rule(RuleViolation::ResolveThroughTheEngineApi))
    ));
    // The broker is told it may not, which is the more useful answer (ADR-0024).
    assert!(matches!(
        w.engine.submit(w.now(), Origin::Broker, release()),
        Err(EngineError::Rule(RuleViolation::OriginNotAllowed { .. }))
    ));
    assert_eq!(w.snapshot()?, before);
    Ok(())
}

// ---- 3. who may decide at each tier -----------------------------------------------------------------

#[test]
fn each_tier_is_decided_by_its_own_decider_or_a_human() -> Res {
    // (tier, decider, allowed): the table of ADR-0012 and ADR-0025.
    let table = [
        (Tier::Routine, Decider::Policy, true),
        (Tier::Routine, Decider::Verifier, false),
        (Tier::Routine, Decider::Human, true),
        (Tier::Escalated, Decider::Policy, false),
        (Tier::Escalated, Decider::Verifier, true),
        (Tier::Escalated, Decider::Human, true),
        (Tier::Irreversible, Decider::Policy, false),
        (Tier::Irreversible, Decider::Verifier, false),
        (Tier::Irreversible, Decider::Human, true),
    ];
    let mut w = world("tiers")?;
    for (tier, decider, allowed) in table {
        let stakes = Stakes {
            tier,
            ..Stakes::routine()
        };
        let h = w.held(stakes)?;
        let before = w.snapshot()?;
        let result = w
            .engine
            .decide_hold(w.now(), h.hold, decider, code("decided.by_tier")?);
        let name = format!("{tier:?} by {decider:?}");
        if allowed {
            result?;
            let r = w.resolved(h.hold).ok_or("not resolved")?;
            let expected = match decider {
                Decider::Policy => Resolution::Policy,
                Decider::Verifier => Resolution::Verifier,
                Decider::Human => Resolution::Human,
            };
            assert_eq!(
                (r.by, r.reason.as_str()),
                (expected, "decided.by_tier"),
                "{name}"
            );
            assert_eq!(
                w.engine.state().tasks()[&h.task].status(),
                TaskStatus::Open,
                "{name}"
            );
        } else {
            assert!(
                matches!(
                    result,
                    Err(EngineError::Rule(RuleViolation::ResolutionNotAuthorized { hold, stakes: s, .. }))
                        if hold == h.hold && s == stakes
                ),
                "{name}: {result:?}"
            );
            assert_eq!(w.snapshot()?, before, "{name}: nothing was written");
            assert!(w.resolved(h.hold).is_none(), "{name}");
        }
    }
    Ok(())
}

macro_rules! the_verifier_cannot_release_a_hold_touching {
    ($test:ident, $category:expr) => {
        #[test]
        fn $test() -> Res {
            let mut w = world(stringify!($test))?;
            for tier in [Tier::Routine, Tier::Escalated, Tier::Irreversible] {
                let stakes = Stakes {
                    tier,
                    ..Stakes::routine()
                }
                .touching($category);
                let h = w.held(stakes)?;
                let before = w.snapshot()?;
                // Neither the verifier nor policy nor a gate allow may release it.
                for decider in [Decider::Verifier, Decider::Policy] {
                    let result =
                        w.engine
                            .decide_hold(w.now(), h.hold, decider, code("decided.no")?);
                    assert!(
                        matches!(
                            result,
                            Err(EngineError::Rule(
                                RuleViolation::ResolutionNotAuthorized { .. }
                            ))
                        ),
                        "{tier:?} {decider:?}: {result:?}"
                    );
                }
                let allow = w.ask_again(&h, says(Answer::Allow), QUICK);
                assert!(
                    matches!(
                        allow,
                        Err(EngineError::Rule(
                            RuleViolation::ResolutionNotAuthorized { .. }
                        ))
                    ),
                    "{tier:?} gate allow: {allow:?}"
                );
                assert_eq!(w.snapshot()?.1, before.1, "{tier:?}: nothing was written");
                // A human can.
                w.engine.decide_hold(
                    w.now(),
                    h.hold,
                    Decider::Human,
                    code("approved.operator")?,
                )?;
                assert_eq!(
                    w.resolved(h.hold).map(|r| r.by),
                    Some(Resolution::Human),
                    "{tier:?}"
                );
            }
            Ok(())
        }
    };
}

the_verifier_cannot_release_a_hold_touching!(
    the_verifier_cannot_release_credentials,
    Sensitive::Credentials
);
the_verifier_cannot_release_a_hold_touching!(the_verifier_cannot_release_policy, Sensitive::Policy);
the_verifier_cannot_release_a_hold_touching!(
    the_verifier_cannot_release_publishing,
    Sensitive::Publishing
);
the_verifier_cannot_release_a_hold_touching!(
    the_verifier_cannot_release_a_capability_grant,
    Sensitive::CapabilityGrant
);

#[test]
fn a_decision_needs_a_hold_that_is_open() -> Res {
    let mut w = world("decide-missing")?;
    let h = w.held(Stakes::routine())?;
    assert!(matches!(
        w.engine.decide_hold(
            w.now(),
            HoldId::from_seq(999),
            Decider::Human,
            ReasonCode::unavailable()
        ),
        Err(EngineError::Rule(RuleViolation::UnknownHold(_)))
    ));
    w.engine
        .decide_hold(w.now(), h.hold, Decider::Policy, ReasonCode::unavailable())?;
    assert!(matches!(
        w.engine
            .decide_hold(w.now(), h.hold, Decider::Human, ReasonCode::unavailable()),
        Err(EngineError::Rule(RuleViolation::HoldNotOpen(_)))
    ));
    Ok(())
}

// ---- replay refuses a release nobody was allowed to make --------------------------------------------

fn raw_event(log: &mut Log, n: u64, origin: Origin, event: &Event) -> Res {
    log.append(
        Timestamp::from_millis(n),
        origin,
        event.kind(),
        SCHEMA_VERSION,
        &event.encode()?,
    )?;
    Ok(())
}

/// A signed log of a task held by an unknown answer with these stakes, then `release`.
fn log_with_release(name: &str, stakes: Stakes, release: Event) -> Res<PathBuf> {
    let p = path(name)?;
    let mut log = Log::open(&p, key())?;
    let task = TaskId::from_seq(1);
    let job = JobId::from_seq(2);
    raw_event(
        &mut log,
        1,
        Origin::Engine,
        &Event::TaskCreated { request: d(1) },
    )?;
    raw_event(
        &mut log,
        2,
        Origin::Engine,
        &Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?;
    raw_event(
        &mut log,
        3,
        Origin::Broker,
        &Event::GateAnswered {
            job,
            gate: Gate::Egress,
            inputs: d(3),
            answer: Answer::Unknown,
            reason: ReasonCode::unavailable(),
        },
    )?;
    raw_event(
        &mut log,
        3,
        Origin::Engine,
        &Event::JobEnded {
            job,
            outcome: JobOutcome::Held,
            reason: ReasonCode::unavailable(),
        },
    )?;
    raw_event(
        &mut log,
        3,
        Origin::Engine,
        &Event::HoldOpened {
            task,
            gate: Gate::Egress,
            inputs: d(3),
            reason: HoldReason::Unknown,
            stakes,
        },
    )?;
    raw_event(&mut log, 4, Origin::Engine, &release)?;
    Ok(p)
}

fn release(by: Resolution) -> Event {
    Event::HoldResolved {
        hold: HoldId::from_seq(5),
        by,
        reason: ReasonCode::unavailable(),
        // A human decision written before signatures existed still replays (ADR-0070).
        signed: if by == Resolution::Human {
            hyalos_types::Signed::Unrecorded
        } else {
            hyalos_types::Signed::None
        },
    }
}

#[test]
fn a_signed_log_that_releases_a_hold_the_wrong_way_is_refused_on_open() -> Res {
    for (name, stakes, by) in [
        (
            "policy-on-irreversible",
            Stakes::irreversible(),
            Resolution::Policy,
        ),
        (
            "verifier-on-credentials",
            Stakes::escalated().touching(Sensitive::Credentials),
            Resolution::Verifier,
        ),
        (
            "allow-on-irreversible",
            Stakes::irreversible(),
            Resolution::Reevaluated,
        ),
        (
            "verifier-on-routine",
            Stakes::routine(),
            Resolution::Verifier,
        ),
    ] {
        let p = log_with_release(name, stakes, release(by))?;
        match Engine::open(&p, key()) {
            Err(EngineError::Replay {
                seq: 6,
                violation: RuleViolation::ResolutionNotAuthorized { .. },
            }) => {}
            other => {
                return Err(
                    format!("{name}: expected a replay refusal at event 6, got {other:?}").into(),
                );
            }
        }
    }
    // The same logs with an allowed decider open fine.
    for (name, stakes, by) in [
        (
            "human-on-irreversible",
            Stakes::irreversible(),
            Resolution::Human,
        ),
        ("policy-on-routine", Stakes::routine(), Resolution::Policy),
        (
            "verifier-on-escalated",
            Stakes::escalated(),
            Resolution::Verifier,
        ),
    ] {
        let p = log_with_release(name, stakes, release(by))?;
        let e = Engine::open(&p, key())?
            .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
        assert_eq!(
            e.state().holds()[&HoldId::from_seq(5)]
                .resolved
                .as_ref()
                .map(|r| r.by),
            Some(by)
        );
    }
    Ok(())
}

// ---- holds written before stakes existed ------------------------------------------------------------

fn old_log(name: &str, by: &str) -> Res<PathBuf> {
    // Version 1 shapes, as the first build wrote them: no stakes, no reason.
    let p = path(name)?;
    let mut log = Log::open(&p, key())?;
    let rows: [(&str, &str); 6] = [
        (
            "task.created",
            &format!(
                r#"{{"kind":"task.created","request":"{}"}}"#,
                "01".repeat(32)
            ),
        ),
        ("job.started", r#"{"kind":"job.started","task":1}"#),
        (
            "gate.answered",
            &format!(
                r#"{{"kind":"gate.answered","job":2,"gate":"egress","inputs":"{}","answer":"unknown","reason":"gate.unavailable"}}"#,
                "03".repeat(32)
            ),
        ),
        (
            "job.ended",
            r#"{"kind":"job.ended","job":2,"outcome":"held"}"#,
        ),
        (
            "hold.opened",
            &format!(
                r#"{{"kind":"hold.opened","task":1,"gate":"egress","inputs":"{}","reason":"unknown"}}"#,
                "03".repeat(32)
            ),
        ),
        (
            "hold.resolved",
            &format!(r#"{{"kind":"hold.resolved","hold":5,"by":"{by}"}}"#),
        ),
    ];
    for (i, (kind, json)) in rows.iter().enumerate() {
        let origin = if *kind == "gate.answered" {
            Origin::Broker
        } else {
            Origin::Engine
        };
        log.append(
            Timestamp::from_millis(1 + i as u64),
            origin,
            kind,
            1,
            json.as_bytes(),
        )?;
    }
    Ok(p)
}

#[test]
fn a_hold_from_before_stakes_existed_reads_as_needing_a_human() -> Res {
    // Written by the first build: no stakes were recorded, so the hold reads as the most cautious.
    let p = old_log("legacy-human", "human")?;
    let e =
        Engine::open(&p, key())?.with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    let hold = &e.state().holds()[&HoldId::from_seq(5)];
    assert_eq!(hold.stakes, Stakes::irreversible());
    let r = hold.resolved.as_ref().ok_or("not resolved")?;
    assert_eq!(
        (r.by, r.reason.as_str()),
        (Resolution::Human, "legacy.unrecorded")
    );

    // A release by anyone else in such a log cannot be shown to have been allowed, so the log is
    // refused rather than trusted (ADR-0025).
    for by in ["policy", "verifier", "reevaluated"] {
        let p = old_log(&format!("legacy-{by}"), by)?;
        assert!(
            matches!(
                Engine::open(&p, key()),
                Err(EngineError::Replay {
                    seq: 6,
                    violation: RuleViolation::ResolutionNotAuthorized { .. }
                })
            ),
            "{by}"
        );
    }
    Ok(())
}

// ---- 5. a hold does not extend a deadline -----------------------------------------------------------

#[test]
fn a_hold_ends_its_job_and_releasing_the_hold_does_not_bring_the_job_back() -> Res {
    let mut w = world("deadline-no-revival")?;
    let h = w.held(Stakes::routine())?;
    let ended = w.engine.state().jobs()[&h.job].ended;
    assert_eq!(ended.map(|e| e.1), Some(JobOutcome::Held));

    w.engine
        .decide_hold(w.now(), h.hold, Decider::Policy, ReasonCode::unavailable())?;
    // The job that was held is over for good, however the hold was released.
    assert_eq!(w.engine.state().jobs()[&h.job].ended, ended);
    let result = w.engine.submit(
        w.now(),
        Origin::Engine,
        Event::JobEnded {
            job: h.job,
            outcome: JobOutcome::Succeeded,
            reason: ReasonCode::unavailable(),
        },
    );
    assert!(matches!(
        result,
        Err(EngineError::Rule(RuleViolation::JobNotRunning(_)))
    ));
    let late = w.engine.rule_gate(
        w.now(),
        Origin::Broker,
        h.job,
        &h.request,
        Outcome::Proceed {
            reason: ReasonCode::unavailable(),
        },
    );
    assert!(matches!(
        late,
        Err(EngineError::Rule(RuleViolation::JobNotRunning(_)))
    ));
    Ok(())
}

#[test]
fn time_passing_releases_nothing() -> Res {
    let mut w = world("deadline-time")?;
    let h = w.held(Stakes::routine())?;
    let before = w.snapshot()?;
    // Longer than any manifest deadline: still held, and no job can start on the task.
    w.clock.advance(Duration::from_secs(60 * 60 * 24 * 90));
    assert_eq!(w.snapshot()?, before);
    assert_eq!(w.engine.state().tasks()[&h.task].status(), TaskStatus::Held);
    let start = w.engine.submit(
        w.now(),
        Origin::Engine,
        Event::JobStarted {
            task: h.task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    );
    assert!(matches!(
        start,
        Err(EngineError::Rule(RuleViolation::TaskNotOpen { .. }))
    ));
    assert_eq!(w.snapshot()?, before);
    // Nor does a restart.
    let reopened = Engine::open(&w.path, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    assert_eq!(reopened.state(), &before.1);
    Ok(())
}

#[test]
fn a_gate_that_reports_after_the_jobs_deadline_can_neither_hold_nor_allow() -> Res {
    let mut w = world("deadline-late-report")?;
    let task = TaskId::from_seq(
        w.engine
            .submit(
                w.now(),
                Origin::Engine,
                Event::TaskCreated { request: d(1) },
            )?
            .seq,
    );
    let job = JobId::from_seq(
        w.engine
            .submit(
                w.now(),
                Origin::Engine,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                    credentials: hyalos_types::CredentialTier::BrokerHeld,
                    attestation: hyalos_types::Attestation::no_hardware_root(),
                },
            )?
            .seq,
    );
    // The deadline passes and the engine ends the job.
    w.engine.submit(
        w.now(),
        Origin::Engine,
        Event::JobEnded {
            job,
            outcome: JobOutcome::DeadlineExceeded,
            reason: ReasonCode::unavailable(),
        },
    )?;
    let before = w.snapshot()?;
    let request = GateRequest {
        gate: Gate::Egress,
        inputs: d(2),
        stakes: Stakes::routine(),
    };
    for outcome in [
        Outcome::Hold {
            reason: ReasonCode::unavailable(),
            why: HoldReason::Unknown,
        },
        Outcome::Proceed {
            reason: ReasonCode::unavailable(),
        },
    ] {
        let result = w
            .engine
            .rule_gate(w.now(), Origin::Broker, job, &request, outcome);
        assert!(
            matches!(
                result,
                Err(EngineError::Rule(RuleViolation::JobNotRunning(_)))
            ),
            "{result:?}"
        );
    }
    assert_eq!(
        w.snapshot()?,
        before,
        "no hold, and no allow, for a job past its deadline"
    );
    Ok(())
}

// ---- unknown never becomes allow by itself: a model over random sequences --------------------------

#[test]
fn no_random_sequence_releases_a_hold_without_an_allow_or_an_authorized_decision() -> Res {
    let (mut released_by_allow, mut released_by_decision, mut refused) = (0, 0, 0);
    for seed in 0..8u64 {
        let sim = Sim::new(seed);
        let mut rng = sim.stream("holds");
        let mut w = world(&format!("model-{seed}"))?;
        w.clock = sim.clock();
        let mut holds: Vec<(Held, Stakes, bool)> = Vec::new(); // (hold, its stakes, released?)

        for step in 0..120 {
            match rng.below(4) {
                0 => {
                    let stakes = match rng.below(3) {
                        0 => Stakes::routine(),
                        1 => Stakes::escalated(),
                        _ => Stakes::irreversible(),
                    };
                    let stakes = if rng.below(4) == 0 {
                        stakes.touching(Sensitive::ALL[rng.below(4) as usize])
                    } else {
                        stakes
                    };
                    holds.push((w.held(stakes)?, stakes, false));
                }
                1 if !holds.is_empty() => {
                    let i = rng.below(holds.len() as u64) as usize;
                    let (answer, takes_secs) = match rng.below(5) {
                        0 => (Ok(Answer::Allow), 0),
                        1 => (Ok(Answer::Allow), 5), // late
                        2 => (Ok(Answer::Unknown), 0),
                        3 => (Ok(Answer::Deny), 0),
                        _ => (Err(Fault::Unavailable), 0),
                    };
                    let result_kind = answer;
                    let result = answer.map_or_else(Err, says);
                    let on_time_allow =
                        matches!(&result, Ok(v) if v.answer == Answer::Allow) && takes_secs == 0;
                    let (held, stakes, done) = &mut holds[i];
                    let outcome = w.ask_again(held, result, Duration::from_secs(takes_secs));
                    if *done {
                        assert!(
                            matches!(
                                outcome,
                                Err(EngineError::Rule(RuleViolation::HoldNotOpen(_)))
                            ),
                            "seed {seed} step {step}"
                        );
                    } else if !on_time_allow {
                        // A refusal is written down as a fact; everything else writes nothing.
                        if matches!(&result_kind, Ok(Answer::Deny)) {
                            assert!(
                                matches!(outcome?, Reevaluation::Refused(_)),
                                "seed {seed} step {step}"
                            );
                        } else {
                            assert_eq!(
                                outcome?,
                                Reevaluation::StillHeld,
                                "seed {seed} step {step}"
                            );
                        }
                    } else if stakes.may_be_resolved_by(Resolution::Reevaluated) {
                        assert!(
                            matches!(outcome?, Reevaluation::Released(_)),
                            "seed {seed} step {step}"
                        );
                        *done = true;
                        released_by_allow += 1;
                    } else {
                        assert!(
                            matches!(
                                outcome,
                                Err(EngineError::Rule(
                                    RuleViolation::ResolutionNotAuthorized { .. }
                                ))
                            ),
                            "seed {seed} step {step}"
                        );
                        refused += 1;
                    }
                }
                2 if !holds.is_empty() => {
                    let i = rng.below(holds.len() as u64) as usize;
                    let (decider, by) = match rng.below(3) {
                        0 => (Decider::Policy, Resolution::Policy),
                        1 => (Decider::Verifier, Resolution::Verifier),
                        _ => (Decider::Human, Resolution::Human),
                    };
                    let (held, stakes, done) = &mut holds[i];
                    let outcome = w.engine.decide_hold(
                        w.now(),
                        held.hold,
                        decider,
                        ReasonCode::unavailable(),
                    );
                    if *done {
                        assert!(matches!(
                            outcome,
                            Err(EngineError::Rule(RuleViolation::HoldNotOpen(_)))
                        ));
                    } else if stakes.may_be_resolved_by(by) {
                        outcome?;
                        *done = true;
                        released_by_decision += 1;
                    } else {
                        assert!(
                            matches!(
                                outcome,
                                Err(EngineError::Rule(
                                    RuleViolation::ResolutionNotAuthorized { .. }
                                ))
                            ),
                            "seed {seed} step {step}"
                        );
                        refused += 1;
                    }
                }
                _ => w.clock.advance(Duration::from_millis(rng.below(3_000))),
            }

            // The engine and the model agree on every hold, and every invariant holds.
            for (held, _, done) in &holds {
                assert_eq!(
                    w.resolved(held.hold).is_some(),
                    *done,
                    "seed {seed} step {step}"
                );
            }
            let broken = w.engine.state().invariant_violations();
            assert!(broken.is_empty(), "seed {seed} step {step}: {broken:?}");
        }
        // Replay agrees.
        let live = w.engine.state().clone();
        drop(w.engine);
        assert_eq!(
            Engine::open(&w.path, key())?
                .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]))
                .state(),
            &live,
            "seed {seed}"
        );
    }
    // Across the seeds the run met every way a hold can be released, and every way it cannot.
    assert!(
        released_by_allow > 0 && released_by_decision > 0 && refused > 0,
        "{released_by_allow} allows, {released_by_decision} decisions, {refused} refusals"
    );
    Ok(())
}
