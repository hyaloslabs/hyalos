//! Cedar schema, baseline policies and evaluation.
//!
//! Layout rule (ADR-0017): this crate depends only on hyalos-types.

mod gate;

pub use gate::{Evaluate, Fault, GateRequest, Outcome, Verdict, ask};
