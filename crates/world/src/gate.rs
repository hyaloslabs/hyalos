//! A gate whose answers come from a seeded stream (ADR-0009, ADR-0013).

use std::time::Duration;

use hyalos_policy::{Evaluate, Fault, GateRequest, Verdict};
use hyalos_sim::FakeClock;
use hyalos_types::{Answer, ReasonCode, SeededRng};

/// Answers allow, deny or unknown, or fails, and takes fake time to do it. `ask` turns any of these
/// but an on-time allow into a refusal or a hold, so a run exercises every path of ADR-0020.
#[derive(Debug)]
pub struct SeededGate {
    rng: SeededRng,
    clock: FakeClock,
    pub asked: u32,
}

impl SeededGate {
    pub fn new(rng: SeededRng, clock: FakeClock) -> Self {
        Self {
            rng,
            clock,
            asked: 0,
        }
    }
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

impl Evaluate for SeededGate {
    fn evaluate(&mut self, _: &GateRequest) -> Result<Verdict, Fault> {
        self.asked += 1;
        // Most answers are quick; a few are slow enough to miss the deadline the caller gives.
        let takes = if self.rng.below(25) == 0 { 400 } else { 5 } + self.rng.below(40);
        self.clock.advance(Duration::from_millis(takes));
        match self.rng.below(40) {
            0 => Err(Fault::Unavailable),
            1 => Err(Fault::TimedOut),
            2 | 3 => Ok(Verdict {
                answer: Answer::Unknown,
                reason: code("sim.unsure"),
            }),
            4..=7 => Ok(Verdict {
                answer: Answer::Deny,
                reason: code("sim.denied"),
            }),
            _ => Ok(Verdict {
                answer: Answer::Allow,
                reason: code("sim.allowed"),
            }),
        }
    }
}
