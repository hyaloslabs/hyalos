//! Cedar schema, baseline policies and evaluation.
//!
//! Layout rule (ADR-0017): this crate depends only on hyalos-types among the workspace crates. Its one
//! external dependency is `cedar-policy` (ADR-0008), whose types stay inside the `cedar` module.

mod cedar;
mod gate;
mod shipped;

pub use cedar::{LoadError, ParseError, Policies, Query, codes};
pub use gate::{Evaluate, Fault, GateRequest, Outcome, Verdict, ask};
pub use shipped::{
    Action, BASELINE, JobFacts, PolicyRequest, SCHEMA, TargetFacts, Tier, tier_group,
};
