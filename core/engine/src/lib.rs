//! The state machine: projections, gates and job lifecycle. Reaches the world only through traits it
//! is given.
//!
//! Layout rule (ADR-0017): this crate must not depend on `hyalos-broker`.

mod audit;
mod engine;
mod mint;
mod state;

pub use audit::{Audit, AuditError, Gap, audit};
pub use engine::{
    Compaction, Decider, Engine, EngineError, PolicyUnavailable, Reevaluation, Ruling,
    pruned_archives, read_state, replay, replay_from, replay_with,
};
pub use mint::{MintRefusal, mint_codes};
pub use state::{
    Cancelled, Hold, Job, PolicyApproval, Pruned, ReleasedHandle, Resolved, RuleViolation, State,
    Task, TaskStatus,
};
