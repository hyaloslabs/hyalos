//! The gate interface (ADR-0009, ADR-0020).
//!
//! Every gate is asked the same way and answers allow, deny or unknown. [`ask`] is the one place that
//! turns what a gate did into what the engine does, and it returns an [`Outcome`], not a `Result`:
//! an error, a timeout or a late answer has no path to `Proceed`, so failing closed does not depend on
//! each caller remembering to handle it.

use hyalos_types::{Answer, Clock, Digest, Gate, HoldReason, ReasonCode, Timestamp};

/// What is being asked. `inputs` is the digest of the request's content; the content stays with the
/// caller, so the log never holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateRequest {
    pub gate: Gate,
    pub inputs: Digest,
}

/// A gate's answer and the short code that says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub answer: Answer,
    pub reason: ReasonCode,
}

/// A gate that could not answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// It reported that it ran out of time.
    TimedOut,
    /// It could not be reached, or failed.
    Unavailable,
}

/// Something that can answer a gate request. Policy evaluation, the egress allowlist, the content
/// gate and a verifier agent all sit behind this.
pub trait Evaluate {
    fn evaluate(&mut self, request: &GateRequest) -> Result<Verdict, Fault>;
}

/// What the engine does next. Only `Proceed` lets the request through.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Proceed {
        reason: ReasonCode,
    },
    /// This one request is refused. The job carries on.
    Refuse {
        reason: ReasonCode,
    },
    /// The task is held until someone decides (ADR-0009, ADR-0012).
    Hold {
        reason: ReasonCode,
        why: HoldReason,
    },
}

/// Asks `gate` and decides what happens. `deadline` is the last moment an answer is still accepted:
/// an answer that arrives after it counts as a timeout, whatever it says, because the caller has
/// already been told the gate did not answer.
pub fn ask(
    gate: &mut dyn Evaluate,
    clock: &dyn Clock,
    deadline: Timestamp,
    request: &GateRequest,
) -> Outcome {
    let timed_out = || Outcome::Hold {
        reason: ReasonCode::timed_out(),
        why: HoldReason::TimedOut,
    };
    if clock.now() > deadline {
        return timed_out();
    }
    let result = gate.evaluate(request);
    if clock.now() > deadline {
        return timed_out();
    }
    match result {
        Ok(Verdict {
            answer: Answer::Allow,
            reason,
        }) => Outcome::Proceed { reason },
        Ok(Verdict {
            answer: Answer::Deny,
            reason,
        }) => Outcome::Refuse { reason },
        Ok(Verdict {
            answer: Answer::Unknown,
            reason,
        }) => Outcome::Hold {
            reason,
            why: HoldReason::Unknown,
        },
        Err(Fault::TimedOut) => timed_out(),
        Err(Fault::Unavailable) => Outcome::Hold {
            reason: ReasonCode::unavailable(),
            why: HoldReason::Unknown,
        },
    }
}
