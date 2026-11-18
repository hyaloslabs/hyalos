//! A gate whose answers come from a seeded stream (ADR-0009, ADR-0013).

use std::time::Duration;

use hyalos_policy::{Evaluate, Fault, GateRequest, Verdict};
use hyalos_sim::FakeClock;
use hyalos_types::{Answer, ReasonCode, SeededRng};

use crate::schedule::{GateMix, Schedule};

/// Answers allow, deny or unknown, or fails, and takes fake time to do it. `ask` turns any of these
/// but an on-time allow into a refusal or a hold, so a run exercises every path of ADR-0020.
#[derive(Debug)]
pub struct SeededGate {
    rng: SeededRng,
    clock: FakeClock,
    mix: GateMix,
    pub asked: u32,
}

impl SeededGate {
    pub fn new(rng: SeededRng, clock: FakeClock) -> Self {
        Self {
            rng,
            clock,
            mix: Schedule::default().gate,
            asked: 0,
        }
    }

    /// Answers as the schedule says instead of at the usual rates.
    pub fn with_mix(mut self, mix: GateMix) -> Self {
        self.mix = mix;
        self
    }
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

impl Evaluate for SeededGate {
    fn evaluate(&mut self, _: &GateRequest) -> Result<Verdict, Fault> {
        self.asked += 1;
        // Most answers are quick; a few are slow enough to miss the deadline the caller gives.
        let takes = if self.mix.slow.hit(&mut self.rng) {
            400
        } else {
            5
        } + self.rng.below(40);
        self.clock.advance(Duration::from_millis(takes));
        let m = self.mix;
        if m.down_after.is_some_and(|after| self.asked > after) {
            return Err(Fault::Unavailable);
        }
        let slot = self.rng.below(m.of);
        let timed_out = m.unavailable + m.timed_out;
        let unknown = timed_out + m.unknown;
        let deny = unknown + m.deny;
        if slot < m.unavailable {
            Err(Fault::Unavailable)
        } else if slot < timed_out {
            Err(Fault::TimedOut)
        } else if slot < unknown {
            Ok(Verdict {
                answer: Answer::Unknown,
                reason: code("sim.unsure"),
            })
        } else if slot < deny {
            Ok(Verdict {
                answer: Answer::Deny,
                reason: code("sim.denied"),
            })
        } else {
            Ok(Verdict {
                answer: Answer::Allow,
                reason: code("sim.allowed"),
            })
        }
    }
}
