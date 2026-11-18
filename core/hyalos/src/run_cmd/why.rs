//! The codes `job.ended` records for why a job ended (ADR-0053). A refusal by the broker is recorded
//! with the broker's own code instead.

/// The harness exited 0.
pub const FINISHED: &str = "harness.finished";
/// A reader or planner step returned its values (ADR-0062).
pub const ANSWERED: &str = "step.answered";
/// The manifest's deadline passed and the sandbox was killed.
pub const DEADLINE: &str = "job.deadline";
/// The sandbox ended any other way: a non-zero exit, or stopped. One its runtime lost holds the task
/// instead, with [`SANDBOX_LOST`] (ADR-0106).
pub const SANDBOX_FAILED: &str = "sandbox.failed";
pub const SANDBOX_NOT_STARTED: &str = "sandbox.not_started";
/// The broker closed its channel while the job ran. Logs written before ADR-0106 carry it; a run now
/// holds the task instead, with [`BROKER_LOST`].
pub const BROKER_STOPPED: &str = "broker.stopped";
/// The broker or its channel was lost while the job ran, so the task holds (ADR-0106).
pub const BROKER_LOST: &str = "broker.lost";
/// The sandbox's runtime lost it while the job ran, so the task holds (ADR-0106).
pub const SANDBOX_LOST: &str = "sandbox.lost";
pub const BROKER_NOT_STARTED: &str = "broker.not_started";
pub const NO_MANIFEST: &str = "manifest.refused";
pub const NO_TOKEN: &str = "forge.no_token";
/// The operator interrupted the run (`SIGINT` or `SIGTERM`).
pub const INTERRUPTED: &str = "run.interrupted";
/// The engine refused to record the operator's release of a handle (ADR-0063).
pub const HANDLE_REFUSED: &str = "handle.refused";
/// The harness exited 0 and its submitted result was accepted (ADR-0064).
pub const SUBMITTED: &str = "step.submitted";
/// The broker did not answer for the job's result, or answered values that do not fit it.
pub const NO_RESULT: &str = "submit.no_answer";
/// The job was running when its runner stopped, and a resumed run ended it (ADR-0069).
pub const RESTARTED: &str = "engine.restarted";
/// The driving agent cancelled the run with `tasks/cancel` (ADR-0069).
pub const CANCELLED: &str = "run.cancelled";
/// The run's deadline passed while the actor waited for the operator to release the issue
/// (ADR-0078).
pub const UNRELEASED: &str = "handle.unreleased";
/// The runner of a run that held the issue stopped, and the broker's copy went with it, so the
/// run is ended rather than resumed (ADR-0078).
pub const LOST: &str = "handle.lost";

pub const ALL: [&str; 19] = [
    FINISHED,
    ANSWERED,
    DEADLINE,
    SANDBOX_FAILED,
    SANDBOX_NOT_STARTED,
    BROKER_STOPPED,
    BROKER_LOST,
    SANDBOX_LOST,
    BROKER_NOT_STARTED,
    NO_MANIFEST,
    NO_TOKEN,
    INTERRUPTED,
    HANDLE_REFUSED,
    SUBMITTED,
    NO_RESULT,
    RESTARTED,
    CANCELLED,
    UNRELEASED,
    LOST,
];
