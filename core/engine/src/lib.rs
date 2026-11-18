//! The state machine: projections, gates and job lifecycle. Reaches the world only through traits it
//! is given.
//!
//! Layout rule (ADR-0017): this crate must not depend on `hyalos-broker`.

mod approvals;
mod audit;
mod engine;
mod learnings;
mod mint;
mod state;

pub use approvals::{ApprovalError, Approvals, Unrecorded, approvals};
pub use audit::{Audit, AuditError, Gap, audit};
pub use engine::{
    Compaction, Decider, Engine, EngineError, PolicyUnavailable, Reevaluation, Ruling,
    pruned_archives, read_state, replay, replay_from, replay_with,
};
pub use learnings::{Learning, Learnings, Lesson, MAX_SOURCES, Source, learn, learnings};
pub use mint::{MintRefusal, mint_codes};
pub use state::{
    Agent, ApprovedMerge, Cancelled, Draft, Hold, ImportedRevocations, ImportedSnapshot,
    InstalledPlugin, Job, PolicyApproval, Pruned, ReleasedHandle, Resolved, Revoked, RevokedPlugin,
    RuleViolation, SetInstallPolicy, State, Task, TaskStatus,
};
