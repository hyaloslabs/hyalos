//! The state machine: projections, gates and job lifecycle. Reaches the world only through traits it
//! is given.
//!
//! Layout rule (ADR-0017): this crate must not depend on `hyalos-broker`.

mod engine;
mod state;

pub use engine::{Engine, EngineError, replay};
pub use state::{Hold, Job, RuleViolation, State, Task, TaskStatus};
