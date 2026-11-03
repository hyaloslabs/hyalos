//! Gates answer allow, deny or unknown, and only an on-time allow proceeds (ADR-0009, ADR-0020).

use std::time::Duration;

use hyalos_policy::{Evaluate, Fault, GateRequest, Outcome, Verdict, ask};
use hyalos_sim::{FakeClock, START};
use hyalos_types::{Answer, Clock, Digest, Gate, HoldReason, ReasonCode, Timestamp};

fn request() -> GateRequest {
    GateRequest {
        gate: Gate::Egress,
        inputs: Digest::from_bytes([4; 32]),
    }
}

fn why(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

fn verdict(answer: Answer) -> Result<Verdict, Fault> {
    Ok(Verdict {
        answer,
        reason: why("test.reason"),
    })
}

/// A gate that returns a fixed result after taking `takes` of fake time.
struct Scripted {
    result: Result<Verdict, Fault>,
    takes: Duration,
    clock: FakeClock,
    asked: u32,
}

impl Scripted {
    fn new(result: Result<Verdict, Fault>, takes: Duration, clock: &FakeClock) -> Self {
        Self {
            result,
            takes,
            clock: clock.clone(),
            asked: 0,
        }
    }
}

impl Evaluate for Scripted {
    fn evaluate(&mut self, _: &GateRequest) -> Result<Verdict, Fault> {
        self.asked += 1;
        self.clock.advance(self.takes);
        self.result.clone()
    }
}

const SECOND: Duration = Duration::from_secs(1);

fn deadline(clock: &FakeClock, after: Duration) -> Timestamp {
    Timestamp::from_millis(clock.now().as_millis() + after.as_millis() as u64)
}

#[test]
fn each_answer_maps_to_one_outcome_and_keeps_its_reason() {
    let clock = FakeClock::new(START);
    let cases = [
        (
            Answer::Allow,
            Outcome::Proceed {
                reason: why("test.reason"),
            },
        ),
        (
            Answer::Deny,
            Outcome::Refuse {
                reason: why("test.reason"),
            },
        ),
        (
            Answer::Unknown,
            Outcome::Hold {
                reason: why("test.reason"),
                why: HoldReason::Unknown,
            },
        ),
    ];
    for (answer, expected) in cases {
        let mut gate = Scripted::new(verdict(answer), Duration::ZERO, &clock);
        let d = deadline(&clock, 5 * SECOND);
        assert_eq!(ask(&mut gate, &clock, d, &request()), expected);
    }
}

#[test]
fn a_gate_that_reports_a_timeout_or_failure_holds() {
    let clock = FakeClock::new(START);
    let mut gate = Scripted::new(Err(Fault::TimedOut), Duration::ZERO, &clock);
    assert_eq!(
        ask(&mut gate, &clock, deadline(&clock, SECOND), &request()),
        Outcome::Hold {
            reason: ReasonCode::timed_out(),
            why: HoldReason::TimedOut
        }
    );
    let mut gate = Scripted::new(Err(Fault::Unavailable), Duration::ZERO, &clock);
    assert_eq!(
        ask(&mut gate, &clock, deadline(&clock, SECOND), &request()),
        Outcome::Hold {
            reason: ReasonCode::unavailable(),
            why: HoldReason::Unknown
        }
    );
}

#[test]
fn a_gate_that_answers_after_the_deadline_has_timed_out_whatever_it_says() {
    let clock = FakeClock::new(START);
    let mut gate = Scripted::new(verdict(Answer::Allow), 3 * SECOND, &clock);
    let outcome = ask(&mut gate, &clock, deadline(&clock, SECOND), &request());
    assert_eq!(gate.asked, 1);
    assert_eq!(
        outcome,
        Outcome::Hold {
            reason: ReasonCode::timed_out(),
            why: HoldReason::TimedOut
        }
    );
}

#[test]
fn an_answer_exactly_at_the_deadline_is_on_time() {
    let clock = FakeClock::new(START);
    let mut gate = Scripted::new(verdict(Answer::Allow), SECOND, &clock);
    let outcome = ask(&mut gate, &clock, deadline(&clock, SECOND), &request());
    assert!(matches!(outcome, Outcome::Proceed { .. }));
    // One millisecond later it is not.
    let mut gate = Scripted::new(
        verdict(Answer::Allow),
        SECOND + Duration::from_millis(1),
        &clock,
    );
    let outcome = ask(&mut gate, &clock, deadline(&clock, SECOND), &request());
    assert!(matches!(
        outcome,
        Outcome::Hold {
            why: HoldReason::TimedOut,
            ..
        }
    ));
}

#[test]
fn a_deadline_that_has_already_passed_holds_without_asking() {
    let clock = FakeClock::new(START);
    let past = clock.now();
    clock.advance(Duration::from_millis(1));
    let mut gate = Scripted::new(verdict(Answer::Allow), Duration::ZERO, &clock);
    let outcome = ask(&mut gate, &clock, past, &request());
    assert_eq!(gate.asked, 0, "an expired request must not reach the gate");
    assert!(matches!(
        outcome,
        Outcome::Hold {
            why: HoldReason::TimedOut,
            ..
        }
    ));
}

#[test]
fn only_an_on_time_allow_ever_proceeds() {
    let results: Vec<Result<Verdict, Fault>> = vec![
        verdict(Answer::Allow),
        verdict(Answer::Deny),
        verdict(Answer::Unknown),
        Err(Fault::TimedOut),
        Err(Fault::Unavailable),
    ];
    let speeds = [
        Duration::ZERO,
        SECOND,
        SECOND + Duration::from_millis(1),
        100 * SECOND,
    ];
    for result in &results {
        for takes in speeds {
            let clock = FakeClock::new(START);
            let mut gate = Scripted::new(result.clone(), takes, &clock);
            let outcome = ask(&mut gate, &clock, deadline(&clock, SECOND), &request());
            let on_time = takes <= SECOND;
            let allowed = matches!(
                result,
                Ok(Verdict {
                    answer: Answer::Allow,
                    ..
                })
            );
            assert_eq!(
                matches!(outcome, Outcome::Proceed { .. }),
                allowed && on_time,
                "{result:?} taking {takes:?} gave {outcome:?}"
            );
            // A refusal needs an on-time deny; nothing else is a refusal.
            let denied = matches!(
                result,
                Ok(Verdict {
                    answer: Answer::Deny,
                    ..
                })
            );
            assert_eq!(matches!(outcome, Outcome::Refuse { .. }), denied && on_time);
        }
    }
}
