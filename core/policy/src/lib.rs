//! Cedar schema, baseline policies and evaluation.
//!
//! Layout rule (ADR-0017): this crate depends only on hyalos-types among the workspace crates. Its one
//! external dependency is `cedar-policy` (ADR-0008), whose types stay inside the `cedar` module.

mod cedar;
mod gate;

pub use cedar::{ParseError, Policies, Query, codes};
pub use gate::{Evaluate, Fault, GateRequest, Outcome, Verdict, ask};
