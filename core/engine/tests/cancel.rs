//! S1.13: a held task can be cancelled by a recorded decision, a gate refusing again is recorded as a
//! fact, and neither releases a hold on its own (ADR-0029).

use std::path::PathBuf;

use hyalos_engine::{
    Decider, Engine, EngineError, Reevaluation, RuleViolation, State, TaskStatus, replay,
};
use hyalos_log::{Log, LogKey};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_types::snapshot::CancelledImage;
use hyalos_types::{
    Digest, Event, Gate, HoldId, HoldReason, JobId, Origin, ReasonCode, Resolution, Schema,
    Sensitive, Stakes, SubstrateName, TaskId, Tier, Timestamp, Touches,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cancel-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

fn put(e: &mut Engine, n: u64, event: Event) -> Res<u64> {
    Ok(e.submit(at(n), Origin::Engine, event)?.seq)
}

/// A new task with one open hold at `stakes`, opened directly.
fn held(e: &mut Engine, n: u64, stakes: Stakes) -> Res<(TaskId, HoldId)> {
    let task = TaskId::from_seq(put(e, n, Event::TaskCreated { request: d(1) })?);
    let hold = HoldId::from_seq(put(e, n + 1, hold_opened(task, stakes))?);
    Ok((task, hold))
}

fn hold_opened(task: TaskId, stakes: Stakes) -> Event {
    Event::HoldOpened {
        task,
        gate: Gate::Egress,
        inputs: d(3),
        reason: HoldReason::Unknown,
        stakes,
    }
}

/// A task held the real way: its job asked a gate and the answer was unknown.
fn held_by_a_job(
    e: &mut Engine,
    n: u64,
    stakes: Stakes,
) -> Res<(TaskId, JobId, HoldId, GateRequest)> {
    let task = TaskId::from_seq(put(e, n, Event::TaskCreated { request: d(1) })?);
    let job = JobId::from_seq(put(
        e,
        n + 1,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
        },
    )?);
    let request = GateRequest {
        gate: Gate::Egress,
        inputs: d(9),
        stakes,
    };
    let ruling = e.rule_gate(
        at(n + 2),
        Origin::Broker,
        job,
        &request,
        Outcome::Hold {
            reason: why(),
            why: HoldReason::Unknown,
        },
    )?;
    let hyalos_engine::Ruling::Held(hold) = ruling else {
        return Err("expected a hold".into());
    };
    Ok((task, job, hold, request))
}

fn rule(result: Result<hyalos_log::Head, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

fn stored(path: &std::path::Path) -> Res<Vec<hyalos_log::StoredEvent>> {
    Ok(Log::open(path, key())?.events()?)
}

#[test]
fn a_held_task_is_ended_by_a_recorded_decision_and_replay_agrees() -> Res {
    let d_ = dir("cancel")?;
    let path = d_.join("events.db");
    let mut e = Engine::open(&path, key())?;
    let (task, _job, hold, request) = held_by_a_job(&mut e, 1, Stakes::routine())?;
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Held);

    let head = e.cancel_task(at(10), task, Decider::Policy, why())?;
    let t = &e.state().tasks()[&task];
    assert_eq!(t.status(), TaskStatus::Cancelled);
    assert!(t.open_holds.is_empty());
    let cancelled = t.cancelled.as_ref().ok_or("not recorded")?;
    assert_eq!(
        (cancelled.at, cancelled.by, cancelled.reason.clone()),
        (at(10), Resolution::Policy, why())
    );
    // The hold was ended, not released.
    assert!(e.state().holds()[&hold].resolved.is_none());
    assert!(!e.state().hold_is_open(&hold));

    // It is a signed engine event of kind task.cancelled, naming who decided.
    let events = stored(&path)?;
    let last = events.last().ok_or("no events")?;
    assert_eq!(
        (last.seq, last.kind.as_str(), last.origin),
        (head.seq, "task.cancelled", Origin::Engine)
    );
    assert!(String::from_utf8_lossy(&last.payload).contains(r#""by":"policy""#));

    // Nothing more can happen to it, and each refusal writes nothing.
    let before = (e.head()?, e.state().clone());
    let cancelled_status = |v| {
        matches!(
            v,
            RuleViolation::TaskNotOpen {
                status: TaskStatus::Cancelled,
                ..
            }
        )
    };
    assert!(cancelled_status(rule(e.submit(
        at(11),
        Origin::Engine,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded)
        }
    ))?));
    assert!(cancelled_status(rule(e.submit(
        at(11),
        Origin::Engine,
        Event::TaskCompleted { task }
    ))?));
    assert_eq!(
        rule(e.submit(at(11), Origin::Engine, hold_opened(task, Stakes::routine())))?,
        RuleViolation::TaskCancelled(task)
    );
    // A later allow from the gate releases nothing, and neither does a decision.
    let allow = || Outcome::Proceed { reason: why() };
    assert_eq!(
        rule_of(e.reevaluate_hold(at(11), hold, &request, allow()))?,
        RuleViolation::HoldNotOpen(hold)
    );
    assert_eq!(
        rule(e.decide_hold(at(11), hold, Decider::Human, why()))?,
        RuleViolation::HoldNotOpen(hold)
    );
    // And it cannot be cancelled twice.
    assert_eq!(
        rule(e.cancel_task(at(11), task, Decider::Human, why()))?,
        RuleViolation::TaskNotHeld {
            task,
            status: TaskStatus::Cancelled
        }
    );
    assert_eq!((e.head()?, e.state().clone()), before);

    // Replaying the log, restarting, and compacting and restarting all give the same state.
    let live = e.state().clone();
    assert_eq!(replay(&stored(&path)?)?, live);
    e.compact(at(20), &d_.join("a.db"))?;
    drop(e);
    let again = Engine::open(&path, key())?;
    assert_eq!(*again.state(), live);
    assert!(again.state().tasks()[&task].cancelled.is_some());
    Ok(())
}

fn rule_of(result: Result<Reevaluation, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

// ---- who may cancel ----------------------------------------------------------------------------

const DECIDERS: [(Decider, Resolution); 3] = [
    (Decider::Policy, Resolution::Policy),
    (Decider::Verifier, Resolution::Verifier),
    (Decider::Human, Resolution::Human),
];

#[test]
fn every_decider_at_every_tier_and_sensitive_category_is_allowed_or_refused_as_the_adr_says() -> Res
{
    let mut stakes_list = Vec::new();
    for tier in [Tier::Routine, Tier::Escalated, Tier::Irreversible] {
        stakes_list.push(Stakes {
            tier,
            touches: Touches::NONE,
        });
        for category in Sensitive::ALL {
            stakes_list.push(Stakes {
                tier,
                touches: Touches::NONE.with(category),
            });
        }
    }
    for (i, stakes) in stakes_list.into_iter().enumerate() {
        for (decider, resolution) in DECIDERS {
            let d_ = dir(&format!("matrix-{i}-{resolution:?}"))?;
            let mut e = Engine::open(&d_.join("events.db"), key())?;
            let (task, _) = held(&mut e, 1, stakes)?;
            let before = (e.head()?, e.state().clone());

            // Written out, not asked of the function under test: a human may cancel anything; policy
            // only a routine hold and the verifier only an escalated one, and neither when anything
            // sensitive is touched.
            let plain = stakes.touches.is_empty();
            let expected = match resolution {
                Resolution::Human => true,
                Resolution::Policy => plain && stakes.tier == Tier::Routine,
                Resolution::Verifier => plain && stakes.tier == Tier::Escalated,
                Resolution::Reevaluated => false,
            };
            let result = e.cancel_task(at(5), task, decider, why());
            let label = format!("{decider:?} at {stakes:?}");
            if expected {
                result?;
                assert_eq!(
                    e.state().tasks()[&task].status(),
                    TaskStatus::Cancelled,
                    "{label}"
                );
            } else {
                assert_eq!(
                    rule(result)?,
                    RuleViolation::CancelNotAuthorized {
                        task,
                        by: resolution,
                        stakes
                    },
                    "{label}"
                );
                assert_eq!(
                    (e.head()?, e.state().clone()),
                    before,
                    "{label}: nothing written"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_task_with_several_holds_needs_a_decider_every_one_of_them_allows() -> Res {
    let d_ = dir("several")?;
    let mut e = Engine::open(&d_.join("events.db"), key())?;
    // Routine and escalated: neither policy nor the verifier is allowed at both; a human is.
    let (task, _) = held(&mut e, 1, Stakes::routine())?;
    put(&mut e, 3, hold_opened(task, Stakes::escalated()))?;
    for (decider, resolution) in [DECIDERS[0], DECIDERS[1]] {
        let err = rule(e.cancel_task(at(5), task, decider, why()))?;
        assert!(
            matches!(err, RuleViolation::CancelNotAuthorized { by, .. } if by == resolution),
            "{err:?}"
        );
    }
    assert_eq!(e.state().tasks()[&task].open_holds.len(), 2);
    e.cancel_task(at(6), task, Decider::Human, why())?;
    assert!(e.state().tasks()[&task].open_holds.is_empty());

    // Two routine holds: policy is allowed at both.
    let (task, _) = held(&mut e, 10, Stakes::routine())?;
    put(&mut e, 12, hold_opened(task, Stakes::routine()))?;
    e.cancel_task(at(14), task, Decider::Policy, why())?;
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Cancelled);
    Ok(())
}

#[test]
fn only_a_held_task_can_be_cancelled() -> Res {
    let d_ = dir("not-held")?;
    let mut e = Engine::open(&d_.join("events.db"), key())?;
    let open = TaskId::from_seq(put(&mut e, 1, Event::TaskCreated { request: d(1) })?);
    let done = TaskId::from_seq(put(&mut e, 2, Event::TaskCreated { request: d(2) })?);
    put(&mut e, 3, Event::TaskCompleted { task: done })?;
    // A task whose only hold was released is open again.
    let (released, hold) = held(&mut e, 4, Stakes::routine())?;
    e.decide_hold(at(6), hold, Decider::Policy, why())?;
    let before = (e.head()?, e.state().clone());

    for (task, status) in [
        (open, TaskStatus::Open),
        (done, TaskStatus::Completed),
        (released, TaskStatus::Open),
    ] {
        assert_eq!(
            rule(e.cancel_task(at(9), task, Decider::Human, why()))?,
            RuleViolation::TaskNotHeld { task, status }
        );
    }
    let nobody = TaskId::from_seq(99);
    assert_eq!(
        rule(e.cancel_task(at(9), nobody, Decider::Human, why()))?,
        RuleViolation::UnknownTask(nobody)
    );
    assert_eq!((e.head()?, e.state().clone()), before);
    Ok(())
}

#[test]
fn a_cancellation_is_recorded_only_through_the_engine_call() -> Res {
    let d_ = dir("api")?;
    let mut e = Engine::open(&d_.join("events.db"), key())?;
    let (task, _) = held(&mut e, 1, Stakes::routine())?;
    let event = || Event::TaskCancelled {
        task,
        by: Resolution::Human,
        reason: why(),
    };
    let before = (e.head()?, e.state().clone());
    let through = RuleViolation::ThroughTheEngineApi {
        kind: "task.cancelled",
    };
    assert_eq!(rule(e.submit(at(5), Origin::Engine, event()))?, through);
    assert_eq!(
        e.submit_all(at(5), &[(Origin::Engine, event())])
            .map_err(|e| e.to_string()),
        Err(EngineError::Rule(through).to_string())
    );
    // The broker is told it may not send it at all.
    assert!(matches!(
        rule(e.submit(at(5), Origin::Broker, event()))?,
        RuleViolation::OriginNotAllowed { .. }
    ));
    assert_eq!((e.head()?, e.state().clone()), before);
    Ok(())
}

// ---- a gate refusing again -----------------------------------------------------------------------

#[test]
fn a_refusal_is_recorded_as_a_fact_and_releases_and_ends_nothing() -> Res {
    let d_ = dir("refusal")?;
    let path = d_.join("events.db");
    let mut e = Engine::open(&path, key())?;
    let (task, _job, hold, request) = held_by_a_job(&mut e, 1, Stakes::routine())?;
    let before = e.state().clone();

    let denied = ReasonCode::new("egress.denied").ok_or("a reason code")?;
    let refuse = || Outcome::Refuse {
        reason: denied.clone(),
    };
    let Reevaluation::Refused(head) = e.reevaluate_hold(at(10), hold, &request, refuse())? else {
        return Err("expected a refusal to be recorded".into());
    };
    let events = stored(&path)?;
    let last = events.last().ok_or("no events")?;
    assert_eq!(
        (last.seq, last.kind.as_str(), last.origin),
        (head.seq, "hold.refused", Origin::Engine)
    );
    assert!(String::from_utf8_lossy(&last.payload).contains(r#""reason":"egress.denied""#));

    // Same tasks, jobs and holds; the hold is open and the task held.
    assert_eq!(e.state().tasks(), before.tasks());
    assert_eq!(e.state().holds(), before.holds());
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Held);
    assert!(e.state().hold_is_open(&hold));

    // Asking again and being refused again is another fact; it never adds up to a decision.
    for n in 11..16 {
        assert!(matches!(
            e.reevaluate_hold(at(n), hold, &request, refuse())?,
            Reevaluation::Refused(_)
        ));
    }
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Held);
    assert_eq!(
        stored(&path)?
            .iter()
            .filter(|x| x.kind == "hold.refused")
            .count(),
        6
    );

    // Unknown and a timeout are not a refusal: nothing is written.
    let head = e.head()?;
    for why_ in [HoldReason::Unknown, HoldReason::TimedOut] {
        let outcome = Outcome::Hold {
            reason: why(),
            why: why_,
        };
        assert_eq!(
            e.reevaluate_hold(at(20), hold, &request, outcome)?,
            Reevaluation::StillHeld
        );
    }
    assert_eq!(e.head()?, head);

    // A later fresh allow still releases it.
    assert!(matches!(
        e.reevaluate_hold(at(21), hold, &request, Outcome::Proceed { reason: why() })?,
        Reevaluation::Released(_)
    ));
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Open);

    // And a refusal replays.
    assert_eq!(replay(&stored(&path)?)?, *e.state());
    Ok(())
}

#[test]
fn a_refusal_is_for_an_open_hold_and_its_own_request_and_goes_through_the_call() -> Res {
    let d_ = dir("refusal-rules")?;
    let mut e = Engine::open(&d_.join("events.db"), key())?;
    let (task, _job, hold, request) = held_by_a_job(&mut e, 1, Stakes::routine())?;
    let refuse = || Outcome::Refuse { reason: why() };
    let before = (e.head()?, e.state().clone());

    // Another gate or other inputs.
    let other = GateRequest {
        inputs: d(200),
        ..request
    };
    assert_eq!(
        rule_of(e.reevaluate_hold(at(10), hold, &other, refuse()))?,
        RuleViolation::ReevaluatedAnotherRequest(hold)
    );
    // No such hold.
    let nobody = HoldId::from_seq(99);
    assert_eq!(
        rule_of(e.reevaluate_hold(at(10), nobody, &request, refuse()))?,
        RuleViolation::UnknownHold(nobody)
    );
    // Not submitted raw, and not by the broker.
    let raw = Event::HoldRefused {
        hold,
        reason: why(),
    };
    assert_eq!(
        rule(e.submit(at(10), Origin::Engine, raw.clone()))?,
        RuleViolation::ThroughTheEngineApi {
            kind: "hold.refused"
        }
    );
    assert!(matches!(
        rule(e.submit(at(10), Origin::Broker, raw))?,
        RuleViolation::OriginNotAllowed { .. }
    ));
    assert_eq!((e.head()?, e.state().clone()), before);

    // Once the hold is ended, whether released or by cancelling its task, a refusal is not recorded.
    e.cancel_task(at(11), task, Decider::Human, why())?;
    assert_eq!(
        rule_of(e.reevaluate_hold(at(12), hold, &request, refuse()))?,
        RuleViolation::HoldNotOpen(hold)
    );
    let (_, _job, released, request) = held_by_a_job(&mut e, 20, Stakes::routine())?;
    e.decide_hold(at(25), released, Decider::Policy, why())?;
    assert_eq!(
        rule_of(e.reevaluate_hold(at(26), released, &request, refuse()))?,
        RuleViolation::HoldNotOpen(released)
    );
    Ok(())
}

#[test]
fn after_a_refusal_a_decision_ends_the_task_and_only_a_decider_allowed_at_its_stakes() -> Res {
    let d_ = dir("refuse-then-decide")?;
    let mut e = Engine::open(&d_.join("events.db"), key())?;
    let (task, _job, hold, request) = held_by_a_job(&mut e, 1, Stakes::escalated())?;
    e.reevaluate_hold(at(10), hold, &request, Outcome::Refuse { reason: why() })?;
    // The gate said no, but that is not a decision; and the wrong tier cannot make one.
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Held);
    assert!(matches!(
        rule(e.cancel_task(at(11), task, Decider::Policy, why()))?,
        RuleViolation::CancelNotAuthorized { .. }
    ));
    e.cancel_task(at(12), task, Decider::Verifier, why())?;
    assert_eq!(e.state().tasks()[&task].status(), TaskStatus::Cancelled);
    Ok(())
}

// ---- the log and the snapshot refuse what the engine would not write ---------------------------

fn raw(log: &mut Log, n: u64, event: &Event) -> Res {
    log.append(
        at(n),
        Origin::Engine,
        event.kind(),
        hyalos_types::event::SCHEMA_VERSION,
        &event.encode()?,
    )?;
    Ok(())
}

#[test]
fn a_log_that_cancels_what_could_not_be_cancelled_does_not_open() -> Res {
    let cases: Vec<(&str, Vec<Event>, &str)> = vec![
        (
            "unauthorised",
            vec![
                Event::TaskCreated { request: d(1) },
                hold_opened(TaskId::from_seq(1), Stakes::escalated()),
                Event::TaskCancelled {
                    task: TaskId::from_seq(1),
                    by: Resolution::Policy,
                    reason: why(),
                },
            ],
            "may not cancel",
        ),
        (
            "by a gate answering again",
            vec![
                Event::TaskCreated { request: d(1) },
                hold_opened(TaskId::from_seq(1), Stakes::routine()),
                Event::TaskCancelled {
                    task: TaskId::from_seq(1),
                    by: Resolution::Reevaluated,
                    reason: why(),
                },
            ],
            "may not cancel",
        ),
        (
            "not held",
            vec![
                Event::TaskCreated { request: d(1) },
                Event::TaskCancelled {
                    task: TaskId::from_seq(1),
                    by: Resolution::Human,
                    reason: why(),
                },
            ],
            "only a held task",
        ),
        (
            "refusal of an ended hold",
            vec![
                Event::TaskCreated { request: d(1) },
                hold_opened(TaskId::from_seq(1), Stakes::routine()),
                Event::TaskCancelled {
                    task: TaskId::from_seq(1),
                    by: Resolution::Human,
                    reason: why(),
                },
                Event::HoldRefused {
                    hold: HoldId::from_seq(2),
                    reason: why(),
                },
            ],
            "not open",
        ),
    ];
    for (name, events, expected) in cases {
        let d_ = dir(&format!("replay-{}", name.replace(' ', "-")))?;
        let path = d_.join("events.db");
        let mut log = Log::open(&path, key())?;
        for (i, event) in events.iter().enumerate() {
            raw(&mut log, i as u64 + 1, event)?;
        }
        drop(log);
        let result = Engine::open(&path, key());
        let message = match result {
            Err(e @ EngineError::Replay { .. }) => e.to_string(),
            other => {
                return Err(
                    format!("{name}: expected a refusal, got {:?}", other.map(|_| ())).into(),
                );
            }
        };
        assert!(message.contains(expected), "{name}: {message}");
    }
    Ok(())
}

#[test]
fn a_build_from_before_the_events_existed_refuses_a_log_that_has_one() -> Res {
    static BEFORE: Schema = Schema {
        current: 3,
        steps: &[],
    };
    let d_ = dir("older")?;
    let path = d_.join("events.db");
    let mut e = Engine::open(&path, key())?;
    let (task, _) = held(&mut e, 1, Stakes::routine())?;
    e.cancel_task(at(5), task, Decider::Human, why())?;
    drop(e);
    let result = Engine::open_with(&path, key(), &BEFORE);
    let message = match result {
        Err(e @ EngineError::Event { .. }) => e.to_string(),
        other => return Err(format!("expected a refusal, got {:?}", other.map(|_| ())).into()),
    };
    assert!(message.contains("newer Hyalos"), "{message}");
    Ok(())
}

#[test]
fn a_snapshot_that_cancels_a_task_it_could_not_have_is_not_started_from() -> Res {
    let d_ = dir("image")?;
    let mut e = Engine::open(&d_.join("events.db"), key())?;
    let (task, _) = held(&mut e, 1, Stakes::escalated())?;
    // A task still held, as the image of a state that would be legal.
    let held_image = e.state().to_image().map_err(|j| format!("{j}"))?;
    assert_eq!(State::from_image(held_image.clone())?, *e.state());
    e.cancel_task(at(5), task, Decider::Verifier, why())?;
    let image = e.state().to_image().map_err(|j| format!("{j}"))?;
    assert_eq!(State::from_image(image.clone())?, *e.state());
    let cancelled = image.tasks[0].cancelled.clone().ok_or("no cancellation")?;

    let with = |f: &dyn Fn(&mut hyalos_types::StateImage)| {
        let mut bad = image.clone();
        f(&mut bad);
        State::from_image(bad)
    };
    // Cancelled by a decider the hold it still has does not allow.
    let error = with(&|i| {
        i.tasks[0].cancelled = Some(CancelledImage {
            by: Resolution::Policy,
            ..cancelled.clone()
        })
    })
    .unwrap_err();
    assert!(error.contains("may not cancel it"), "{error}");
    // Cancelled and completed.
    let error = with(&|i| i.tasks[0].completed_at = Some(9)).unwrap_err();
    assert!(error.contains("both completed and cancelled"), "{error}");
    // Cancelled but still listing an open hold.
    let error = with(&|i| i.tasks[0].open_holds = vec![i.holds[0].id]).unwrap_err();
    assert!(error.contains("wrong open holds"), "{error}");
    Ok(())
}
