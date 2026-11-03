//! The state machine: projections, gates and job lifecycle. Reaches the world only through traits it
//! is given.
//!
//! Layout rule (ADR-0017): this crate must not depend on `hyalos-broker`.

mod engine;
mod state;

pub use engine::{Decider, Engine, EngineError, Reevaluation, Ruling, replay, replay_with};
pub use state::{Hold, Job, Resolved, RuleViolation, State, Task, TaskStatus};
