//! Tasks, jobs, holds and agents' credentials, rebuilt from events (ADR-0003, ADR-0019, ADR-0068).
//!
//! [`State`] is a pure fold over events. [`State::apply`] checks that an event is legal for the state it
//! lands on and refuses it otherwise, so an engine bug or a damaged log shows up as an error, not as
//! silently wrong state. Everything here is ordered (`BTreeMap`), so two replays of one log are equal.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use hyalos_types::snapshot::{
    AgentImage, CancelledImage, HandleImage, HoldImage, JobEndImage, JobImage, MergeImage,
    PolicyImage, PrunedImage, ResolvedImage, RevokedImage, StateImage, TaskImage,
};
use hyalos_types::workflow::MAX_FIELDS;
use hyalos_types::{
    AgentId, AgentName, Answer, CommitSha, Digest, Event, Gate, HoldId, HoldReason, JobId,
    JobOutcome, MAX_POLICY_DOCUMENT, MergeMethod, Origin, ReasonCode, Resolution, SIGNED_SINCE,
    Sensitive, Signed, Stakes, TaskId, Timestamp, TypedValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Open,
    /// At least one hold is open (ADR-0009).
    Held,
    Completed,
    /// Ended by a decision while held (ADR-0029). Nothing more happens to it.
    Cancelled,
}

/// A held task that was ended by a decision (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancelled {
    pub at: Timestamp,
    pub by: Resolution,
    pub reason: ReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub created_at: Timestamp,
    pub request: Digest,
    pub completed_at: Option<Timestamp>,
    pub open_holds: BTreeSet<HoldId>,
    pub cancelled: Option<Cancelled>,
}

impl Task {
    pub fn status(&self) -> TaskStatus {
        if self.cancelled.is_some() {
            TaskStatus::Cancelled
        } else if self.completed_at.is_some() {
            TaskStatus::Completed
        } else if !self.open_holds.is_empty() {
            TaskStatus::Held
        } else {
            TaskStatus::Open
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub task: TaskId,
    pub started_at: Timestamp,
    pub ended: Option<(Timestamp, JobOutcome)>,
    /// Why it ended, once it has (ADR-0053). A job from an image older than version 7 says
    /// `legacy.unrecorded`.
    pub reason: Option<ReasonCode>,
    /// The typed values it returned, if any (ADR-0069).
    pub values: Option<Vec<TypedValue>>,
}

impl Job {
    pub fn is_running(&self) -> bool {
        self.ended.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub task: TaskId,
    pub gate: Gate,
    pub inputs: Digest,
    pub reason: HoldReason,
    /// How much the held action matters, which decides who may release it (ADR-0025).
    pub stakes: Stakes,
    pub opened_at: Timestamp,
    pub resolved: Option<Resolved>,
}

/// How and when a hold was released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub at: Timestamp,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// An event that is not legal for the state it would land on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleViolation {
    UnknownTask(TaskId),
    UnknownJob(JobId),
    UnknownHold(HoldId),
    /// A job can only start on, and a task can only complete from, an open task.
    TaskNotOpen {
        task: TaskId,
        status: TaskStatus,
    },
    /// A completed task takes no more holds.
    TaskCompleted(TaskId),
    /// A task has at most one running job, and it cannot be held or completed while one runs: end the
    /// job first (ADR-0019).
    TaskHasRunningJob {
        task: TaskId,
        job: JobId,
    },
    JobNotRunning(JobId),
    HoldNotOpen(HoldId),
    /// Events must be applied in increasing sequence order.
    OutOfOrder {
        seq: u64,
        last: u64,
    },
    /// A gate answered unknown, so the next events must end its job as held and open the hold for
    /// that gate and those inputs, and nothing else may come between (ADR-0020).
    HoldRequired {
        job: JobId,
        next: &'static str,
    },
    /// An unknown answer must be written together with its hold, never alone (ADR-0020).
    UnknownNeedsHold,
    /// This origin may not submit this kind of event (ADR-0024).
    OriginNotAllowed {
        kind: &'static str,
        origin: Origin,
    },
    /// This decider may not release a hold with these stakes (ADR-0025).
    ResolutionNotAuthorized {
        hold: HoldId,
        by: Resolution,
        stakes: Stakes,
    },
    /// A hold is released through `Engine::reevaluate_hold` or `Engine::decide_hold`, which record
    /// what justifies it; a raw `hold.resolved` is refused (ADR-0025).
    ResolveThroughTheEngineApi,
    /// A re-evaluation asked a different gate, or about different inputs, than the hold holds.
    ReevaluatedAnotherRequest(HoldId),
    /// An event is stamped earlier than the one before it (ADR-0024).
    TimeWentBackwards {
        previous: Timestamp,
        at: Timestamp,
    },
    /// The log ends after an unknown answer, before its hold. Nothing may proceed on unknown.
    UnfinishedHold(JobId),
    /// Only a held task can be cancelled (ADR-0029).
    TaskNotHeld {
        task: TaskId,
        status: TaskStatus,
    },
    /// This decider may not cancel a task with an open hold at these stakes (ADR-0029).
    CancelNotAuthorized {
        task: TaskId,
        by: Resolution,
        stakes: Stakes,
    },
    /// A cancelled task takes no more holds.
    TaskCancelled(TaskId),
    /// This kind of event is written by an engine call that justifies it, never submitted raw.
    ThroughTheEngineApi {
        kind: &'static str,
    },
    /// An archive that ends at an event the log has not reached cannot have been made from it.
    ArchiveBeyondLog {
        last_seq: u64,
        log_end: u64,
    },
    /// That archive was already recorded as pruned (ADR-0026).
    AlreadyPruned {
        last_seq: u64,
    },
    /// No snapshot of this log stands at that event with that hash, so an archive ending there is not
    /// one this log made.
    NotASnapshotOfThisLog {
        last_seq: u64,
    },
    /// A policy change is irreversible and touches policy, so only a human approves it (ADR-0034).
    PolicyNeedsAHuman {
        by: Resolution,
    },
    /// A policy document is bounded (ADR-0034).
    PolicyTooLarge {
        len: usize,
    },
    /// Raw forge text reaches an actor only by a human's decision (ADR-0012, ADR-0063).
    HandleNeedsAHuman {
        by: Resolution,
    },
    /// That handle was already released to that task.
    HandleAlreadyReleased {
        task: TaskId,
        handle: Digest,
    },
    /// A merge is irreversible, so only a human approves one (ADR-0012, ADR-0071).
    MergeNeedsAHuman {
        by: Resolution,
    },
    /// A task holds one merge approval, made before any job runs on it.
    MergeAlreadyApproved(TaskId),
    /// The repository or pull request number is not one an approval may name.
    MergeTargetInvalid,
    /// Issuing a credential grants a capability, so only a human does it (ADR-0012, ADR-0068).
    AgentNeedsAHuman {
        by: Resolution,
    },
    /// Two credentials never share a secret, revoked or not, so a revoked one cannot come back.
    AgentCredentialReused,
    /// A credential that is not revoked already has that name.
    AgentNameInUse(AgentName),
    UnknownAgent(AgentId),
    AgentAlreadyRevoked(AgentId),
    /// A revocation is a decision, never a gate's re-evaluation.
    RevokeNotADecision,
    /// A human decision must carry the operator's signature (ADR-0070).
    HumanDecisionUnsigned,
    /// Only a human decision carries a signature.
    SignedButNotHuman {
        by: Resolution,
    },
    /// The approval names an event after the last one, or before the last decision the operator
    /// signed, so it was not signed for this place in the log.
    ApprovalOutOfPlace {
        after: u64,
        since: u64,
        last: u64,
    },
    /// A job returns its values once, while it runs, and no more of them than an output has fields.
    JobValuesTwice(JobId),
    TooManyValues(JobId),
}

impl fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleViolation::UnknownTask(t) => write!(f, "task {t} does not exist"),
            RuleViolation::UnknownJob(j) => write!(f, "job {j} does not exist"),
            RuleViolation::UnknownHold(h) => write!(f, "hold {h} does not exist"),
            RuleViolation::TaskNotOpen { task, status } => {
                write!(f, "task {task} is {status:?}, not open")
            }
            RuleViolation::TaskCompleted(t) => write!(f, "task {t} is already completed"),
            RuleViolation::TaskHasRunningJob { task, job } => {
                write!(
                    f,
                    "task {task} still has job {job} running; end the job first"
                )
            }
            RuleViolation::JobNotRunning(j) => write!(f, "job {j} is not running"),
            RuleViolation::HoldNotOpen(h) => write!(f, "hold {h} is not open"),
            RuleViolation::HoldRequired { job, next } => write!(
                f,
                "job {job}'s gate answered unknown, so the next event must be {next}"
            ),
            RuleViolation::UnknownNeedsHold => write!(
                f,
                "an unknown answer is written together with its hold, not on its own"
            ),
            RuleViolation::UnfinishedHold(job) => write!(
                f,
                "the log ends after job {job}'s gate answered unknown, before its hold opened"
            ),
            RuleViolation::ArchiveBeyondLog { last_seq, log_end } => write!(
                f,
                "an archive ending at event {last_seq} cannot come from a log that ends at event {log_end}"
            ),
            RuleViolation::TaskNotHeld { task, status } => {
                write!(
                    f,
                    "task {task} is {status:?}, and only a held task can be cancelled"
                )
            }
            RuleViolation::CancelNotAuthorized { task, by, stakes } => write!(
                f,
                "{by:?} may not cancel task {task}, which has a hold at {stakes:?}"
            ),
            RuleViolation::TaskCancelled(t) => write!(f, "task {t} is cancelled"),
            RuleViolation::ThroughTheEngineApi { kind } => write!(
                f,
                "{kind} is recorded by the engine call that justifies it (cancel_task, reevaluate_hold, approve_policy, release_handle, issue_agent or revoke_agent), not by submitting it"
            ),
            RuleViolation::PolicyNeedsAHuman { by } => write!(
                f,
                "a policy change needs a human's approval; {by:?} may not approve one (ADR-0012, ADR-0034)"
            ),
            RuleViolation::HandleNeedsAHuman { by } => write!(
                f,
                "releasing a handle's text to an actor needs a human's decision; {by:?} may not make one (ADR-0012, ADR-0063)"
            ),
            RuleViolation::HandleAlreadyReleased { task, handle } => {
                write!(f, "handle {handle} is already released to task {task}")
            }
            RuleViolation::MergeNeedsAHuman { by } => write!(
                f,
                "a merge needs a human's approval; {by:?} may not approve one (ADR-0012, ADR-0071)"
            ),
            RuleViolation::MergeAlreadyApproved(task) => write!(
                f,
                "task {task} already has a merge approved, or has started a job; a merge approval opens its own task"
            ),
            RuleViolation::MergeTargetInvalid => f.write_str(
                "a merge approval names an owner/name repository and a pull request number",
            ),
            RuleViolation::AgentNeedsAHuman { by } => write!(
                f,
                "issuing an agent's credential needs a human; {by:?} may not issue one (ADR-0012, ADR-0068)"
            ),
            RuleViolation::AgentCredentialReused => {
                write!(f, "that credential was already issued")
            }
            RuleViolation::AgentNameInUse(name) => write!(
                f,
                "an agent's credential named {name} is already in force; revoke it first"
            ),
            RuleViolation::UnknownAgent(a) => write!(f, "agent {a} does not exist"),
            RuleViolation::AgentAlreadyRevoked(a) => write!(f, "agent {a} is already revoked"),
            RuleViolation::JobValuesTwice(j) => {
                write!(f, "job {j} has already returned its values")
            }
            RuleViolation::TooManyValues(j) => write!(
                f,
                "job {j} returned more values than an output has fields ({MAX_FIELDS})"
            ),
            RuleViolation::RevokeNotADecision => {
                write!(f, "a credential is revoked by a decision, not by a gate")
            }
            RuleViolation::HumanDecisionUnsigned => {
                write!(f, "a human decision must carry the operator's signature")
            }
            RuleViolation::SignedButNotHuman { by } => {
                write!(f, "a decision by {by:?} carries no operator signature")
            }
            RuleViolation::ApprovalOutOfPlace { after, since, last } => write!(
                f,
                "the approval was signed after event {after}, which is not between the last \
                 approval ({since}) and the last event ({last})"
            ),
            RuleViolation::PolicyTooLarge { len } => write!(
                f,
                "a policy document of {len} bytes is larger than the {MAX_POLICY_DOCUMENT} bytes allowed"
            ),
            RuleViolation::NotASnapshotOfThisLog { last_seq } => write!(
                f,
                "no snapshot of this log stands at event {last_seq} with that hash, so an archive ending there is not one it made"
            ),
            RuleViolation::AlreadyPruned { last_seq } => write!(
                f,
                "the archive ending at event {last_seq} is already recorded as pruned"
            ),
            RuleViolation::OutOfOrder { seq, last } => {
                write!(f, "event {seq} arrived after event {last}")
            }
            RuleViolation::ResolutionNotAuthorized { hold, by, stakes } => write!(
                f,
                "hold {hold} ({:?}, touching {:?}) may not be released by {by:?} (ADR-0025)",
                stakes.tier, stakes.touches
            ),
            RuleViolation::ResolveThroughTheEngineApi => write!(
                f,
                "a hold is released through reevaluate_hold or decide_hold, not by submitting hold.resolved"
            ),
            RuleViolation::ReevaluatedAnotherRequest(h) => write!(
                f,
                "that answer is for a different gate or different inputs than hold {h} holds"
            ),
            RuleViolation::OriginNotAllowed { kind, origin } => write!(
                f,
                "a `{kind}` event may not come from the {} (ADR-0024)",
                origin.as_str()
            ),
            RuleViolation::TimeWentBackwards { previous, at } => write!(
                f,
                "an event stamped {} ms comes after one stamped {} ms; log time does not go backwards",
                at.as_millis(),
                previous.as_millis()
            ),
        }
    }
}

impl std::error::Error for RuleViolation {}

/// What an unknown answer still owes: the job ending held, then the hold opening.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    job: JobId,
    task: TaskId,
    gate: Gate,
    inputs: Digest,
    job_ended: bool,
}

/// Every task, job and hold, as of `last_seq`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    tasks: BTreeMap<TaskId, Task>,
    jobs: BTreeMap<JobId, Job>,
    holds: BTreeMap<HoldId, Hold>,
    pruned: Vec<Pruned>,
    policy: Option<PolicyApproval>,
    handles: Vec<ReleasedHandle>,
    agents: BTreeMap<AgentId, Agent>,
    merges: Vec<ApprovedMerge>,
    pending: Option<Pending>,
    /// The event of the last decision the operator signed (ADR-0070).
    last_approval: Option<u64>,
    /// The first event written at schema version 13 or later (ADR-0070).
    signed_since: Option<u64>,
    last_seq: u64,
    last_at: Option<Timestamp>,
}

/// The operator policy in force: the document from the latest `policy.approved` event (ADR-0034).
/// Before any, there is none, and only the shipped baseline applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyApproval {
    /// The event that approved it.
    pub seq: u64,
    pub at: Timestamp,
    pub by: Resolution,
    pub reason: ReasonCode,
    pub document: String,
}

/// A handle a human released to a task's jobs (ADR-0063).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasedHandle {
    /// The event that released it.
    pub seq: u64,
    pub at: Timestamp,
    pub task: TaskId,
    pub handle: Digest,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// A merge a human approved for a task (ADR-0071): pull request `pr` of `repo` by `method`, only while
/// its head is `head`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedMerge {
    /// The event that approved it.
    pub seq: u64,
    pub at: Timestamp,
    pub task: TaskId,
    pub repo: String,
    pub pr: u64,
    pub head: CommitSha,
    pub method: MergeMethod,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// A credential a human issued to a driving agent (ADR-0068). The MCP server admits a caller who
/// presents a secret whose SHA-256 is `credential`, until it is revoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub at: Timestamp,
    pub name: AgentName,
    pub credential: Digest,
    pub by: Resolution,
    pub reason: ReasonCode,
    pub revoked: Option<Revoked>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revoked {
    pub at: Timestamp,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// An archive that was deleted from the machine, as the log recorded it (ADR-0026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pruned {
    /// The event that recorded it.
    pub seq: u64,
    pub at: Timestamp,
    /// The last event the archive held, and its hash.
    pub last_seq: u64,
    pub last_hash: Digest,
}

impl State {
    pub fn tasks(&self) -> &BTreeMap<TaskId, Task> {
        &self.tasks
    }

    pub fn jobs(&self) -> &BTreeMap<JobId, Job> {
        &self.jobs
    }

    pub fn holds(&self) -> &BTreeMap<HoldId, Hold> {
        &self.holds
    }

    /// Every handle released, in the order it was recorded (ADR-0063).
    pub fn handles(&self) -> &[ReleasedHandle] {
        &self.handles
    }

    /// Every merge approved, in the order it was recorded (ADR-0071).
    pub fn merges(&self) -> &[ApprovedMerge] {
        &self.merges
    }

    /// The merge approved for `task`, if one was.
    pub fn merge_of(&self, task: TaskId) -> Option<&ApprovedMerge> {
        self.merges.iter().find(|m| m.task == task)
    }

    /// Whether `handle` was released to `task`.
    pub fn handle_is_released(&self, task: TaskId, handle: &Digest) -> bool {
        self.handles
            .iter()
            .any(|h| h.task == task && h.handle == *handle)
    }

    /// Every agent's credential ever issued, revoked ones too (ADR-0068).
    pub fn agents(&self) -> &BTreeMap<AgentId, Agent> {
        &self.agents
    }

    /// The agent whose credential has this SHA-256 and is not revoked, if there is one. This is the
    /// whole of the MCP server's admission (ADR-0068).
    pub fn admitted_agent(&self, credential: &Digest) -> Option<AgentId> {
        self.agents
            .iter()
            .find(|(_, a)| a.revoked.is_none() && a.credential == *credential)
            .map(|(id, _)| *id)
    }

    /// The operator policy in force, if one was ever approved.
    pub fn policy(&self) -> Option<&PolicyApproval> {
        self.policy.as_ref()
    }

    /// Every archive recorded as pruned, in the order it was recorded.
    pub fn pruned(&self) -> &[Pruned] {
        &self.pruned
    }

    /// Whether `hold` exists and is still undecided: not released, and its task not cancelled.
    pub fn hold_is_open(&self, hold: &HoldId) -> bool {
        self.holds.get(hold).is_some_and(|h| {
            h.resolved.is_none()
                && self
                    .tasks
                    .get(&h.task)
                    .is_some_and(|t| t.cancelled.is_none())
        })
    }

    /// Whether the archive that ends at `last_seq` with `last_hash` is recorded as pruned.
    pub fn is_pruned(&self, last_seq: u64, last_hash: &Digest) -> bool {
        self.pruned
            .iter()
            .any(|p| p.last_seq == last_seq && p.last_hash == *last_hash)
    }

    /// The sequence number of the last event applied, or 0 for none.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// The stamp of the most recent event, if there is one.
    pub fn last_at(&self) -> Option<Timestamp> {
        self.last_at
    }

    fn task(&self, id: TaskId) -> Result<&Task, RuleViolation> {
        self.tasks.get(&id).ok_or(RuleViolation::UnknownTask(id))
    }

    fn running_job_of(&self, task: TaskId) -> Option<JobId> {
        self.jobs
            .iter()
            .find(|(_, j)| j.task == task && j.is_running())
            .map(|(id, _)| *id)
    }

    /// Whether an unknown answer is still waiting for its hold. Between operations this is false; the
    /// engine writes the answer, the job's end and the hold in one transaction (ADR-0020).
    pub fn awaiting_hold(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether `event`, stamped `at` and submitted by `origin`, is legal now. It changes nothing.
    /// Time may repeat, since events written together share a stamp, but never go back (ADR-0024).
    pub fn check_stamped(
        &self,
        at: Timestamp,
        origin: Origin,
        event: &Event,
    ) -> Result<(), RuleViolation> {
        if !event.may_come_from(origin) {
            return Err(RuleViolation::OriginNotAllowed {
                kind: event.kind(),
                origin,
            });
        }
        if let Some(previous) = self.last_at
            && at < previous
        {
            return Err(RuleViolation::TimeWentBackwards { previous, at });
        }
        self.check(event)
    }

    /// Whether `event` is legal now, whoever sent it and whenever. It changes nothing.
    pub fn check(&self, event: &Event) -> Result<(), RuleViolation> {
        if let Some(p) = &self.pending {
            let expected = match (p.job_ended, event) {
                (
                    false,
                    Event::JobEnded {
                        job,
                        outcome: JobOutcome::Held,
                        ..
                    },
                ) if *job == p.job => None,
                (
                    true,
                    Event::HoldOpened {
                        task, gate, inputs, ..
                    },
                ) if *task == p.task && *gate == p.gate && *inputs == p.inputs => None,
                (false, _) => Some("job.ended with outcome held for that job"),
                (true, _) => Some("hold.opened for that task, gate and inputs"),
            };
            if let Some(next) = expected {
                return Err(RuleViolation::HoldRequired { job: p.job, next });
            }
        }
        self.check_signed(event)?;
        self.check_rules(event)
    }

    /// A human decision carries the operator's signature and any other carries none (ADR-0070). The
    /// signature itself is checked against `operator.pub` by the engine and by `hyalos log verify`;
    /// here, where no key is known, only its place is: it names an event at or before the last one,
    /// and at or after the last decision the operator signed, so an old approval cannot be written
    /// again. A human decision from before schema version 13 reads as unrecorded and is let through.
    fn check_signed(&self, event: &Event) -> Result<(), RuleViolation> {
        let (Some(by), Some(signed)) = (event.decided_by(), event.signed()) else {
            return Ok(());
        };
        match (by, signed) {
            (Resolution::Human, Signed::Operator(approval)) => {
                let since = self.last_approval.unwrap_or(0);
                if approval.after > self.last_seq || approval.after < since {
                    return Err(RuleViolation::ApprovalOutOfPlace {
                        after: approval.after,
                        since,
                        last: self.last_seq,
                    });
                }
                Ok(())
            }
            (Resolution::Human, Signed::Unrecorded) => Ok(()),
            (Resolution::Human, Signed::None) => Err(RuleViolation::HumanDecisionUnsigned),
            (_, Signed::None) => Ok(()),
            (by, _) => Err(RuleViolation::SignedButNotHuman { by }),
        }
    }

    fn check_rules(&self, event: &Event) -> Result<(), RuleViolation> {
        match event {
            Event::TaskCreated { .. } => Ok(()),
            Event::TaskCompleted { task } => {
                let status = self.task(*task)?.status();
                if status != TaskStatus::Open {
                    return Err(RuleViolation::TaskNotOpen {
                        task: *task,
                        status,
                    });
                }
                self.no_running_job(*task)
            }
            Event::JobStarted { task, .. } => {
                let status = self.task(*task)?.status();
                if status != TaskStatus::Open {
                    return Err(RuleViolation::TaskNotOpen {
                        task: *task,
                        status,
                    });
                }
                // One running job per task. Concurrent jobs would need their own decision (ADR-0019).
                self.no_running_job(*task)
            }
            Event::GateAnswered { job, .. } => match self.jobs.get(job) {
                None => Err(RuleViolation::UnknownJob(*job)),
                Some(j) if !j.is_running() => Err(RuleViolation::JobNotRunning(*job)),
                Some(_) => Ok(()),
            },
            Event::JobEnded { job, .. } => match self.jobs.get(job) {
                None => Err(RuleViolation::UnknownJob(*job)),
                Some(j) if !j.is_running() => Err(RuleViolation::JobNotRunning(*job)),
                Some(_) => Ok(()),
            },
            Event::JobValues { job, values } => match self.jobs.get(job) {
                None => Err(RuleViolation::UnknownJob(*job)),
                Some(j) if !j.is_running() => Err(RuleViolation::JobNotRunning(*job)),
                Some(j) if j.values.is_some() => Err(RuleViolation::JobValuesTwice(*job)),
                Some(_) if values.len() > MAX_FIELDS => Err(RuleViolation::TooManyValues(*job)),
                Some(_) => Ok(()),
            },
            Event::HoldOpened { task, .. } => {
                match self.task(*task)?.status() {
                    TaskStatus::Completed => return Err(RuleViolation::TaskCompleted(*task)),
                    TaskStatus::Cancelled => return Err(RuleViolation::TaskCancelled(*task)),
                    TaskStatus::Open | TaskStatus::Held => {}
                }
                self.no_running_job(*task)
            }
            Event::HoldRefused { hold, .. } => {
                if !self.holds.contains_key(hold) {
                    return Err(RuleViolation::UnknownHold(*hold));
                }
                if !self.hold_is_open(hold) {
                    return Err(RuleViolation::HoldNotOpen(*hold));
                }
                Ok(())
            }
            Event::TaskCancelled { task, by, .. } => {
                let t = self.task(*task)?;
                if t.status() != TaskStatus::Held {
                    return Err(RuleViolation::TaskNotHeld {
                        task: *task,
                        status: t.status(),
                    });
                }
                // Every open hold must allow this decider, so a lower one cannot end work that a
                // higher one is to decide.
                for hold in &t.open_holds {
                    if let Some(h) = self.holds.get(hold)
                        && !h.stakes.may_be_cancelled_by(*by)
                    {
                        return Err(RuleViolation::CancelNotAuthorized {
                            task: *task,
                            by: *by,
                            stakes: h.stakes,
                        });
                    }
                }
                Ok(())
            }
            Event::HoldResolved { hold, by, .. } => match self.holds.get(hold) {
                None => Err(RuleViolation::UnknownHold(*hold)),
                Some(_) if !self.hold_is_open(hold) => Err(RuleViolation::HoldNotOpen(*hold)),
                Some(h) if !h.stakes.may_be_resolved_by(*by) => {
                    Err(RuleViolation::ResolutionNotAuthorized {
                        hold: *hold,
                        by: *by,
                        stakes: h.stakes,
                    })
                }
                Some(_) => Ok(()),
            },
            Event::PolicyApproved { document, by, .. } => {
                // The same authority table as releasing a hold (ADR-0025): a change of policy is
                // irreversible and touches policy, so it is a human's.
                if !Stakes::irreversible()
                    .touching(Sensitive::Policy)
                    .may_be_resolved_by(*by)
                {
                    return Err(RuleViolation::PolicyNeedsAHuman { by: *by });
                }
                if document.len() > MAX_POLICY_DOCUMENT {
                    return Err(RuleViolation::PolicyTooLarge {
                        len: document.len(),
                    });
                }
                Ok(())
            }
            Event::HandleReleased {
                task, handle, by, ..
            } => {
                match self.task(*task)?.status() {
                    TaskStatus::Completed => return Err(RuleViolation::TaskCompleted(*task)),
                    TaskStatus::Cancelled => return Err(RuleViolation::TaskCancelled(*task)),
                    TaskStatus::Open | TaskStatus::Held => {}
                }
                // The authority table of a hold (ADR-0025) at the irreversible tier: a human only.
                if !Stakes::irreversible().may_be_resolved_by(*by) {
                    return Err(RuleViolation::HandleNeedsAHuman { by: *by });
                }
                if self.handle_is_released(*task, handle) {
                    return Err(RuleViolation::HandleAlreadyReleased {
                        task: *task,
                        handle: *handle,
                    });
                }
                Ok(())
            }
            Event::MergeApproved {
                task, repo, pr, by, ..
            } => {
                let status = self.task(*task)?.status();
                if status != TaskStatus::Open {
                    return Err(RuleViolation::TaskNotOpen {
                        task: *task,
                        status,
                    });
                }
                // The authority table (ADR-0025) at the irreversible tier: a human only.
                if !Stakes::irreversible().may_be_resolved_by(*by) {
                    return Err(RuleViolation::MergeNeedsAHuman { by: *by });
                }
                if !hyalos_types::merge::valid_repo(repo) || !hyalos_types::merge::valid_pr(*pr) {
                    return Err(RuleViolation::MergeTargetInvalid);
                }
                // One approval a task, recorded before the task's merge job, so the job's manifest has
                // exactly one approval to be minted from.
                if self.merge_of(*task).is_some() || self.jobs.values().any(|j| j.task == *task) {
                    return Err(RuleViolation::MergeAlreadyApproved(*task));
                }
                Ok(())
            }
            Event::AgentIssued {
                name,
                credential,
                by,
                ..
            } => {
                // The authority table (ADR-0025) for an irreversible capability grant: a human only.
                if !Stakes::irreversible()
                    .touching(Sensitive::CapabilityGrant)
                    .may_be_resolved_by(*by)
                {
                    return Err(RuleViolation::AgentNeedsAHuman { by: *by });
                }
                if self.agents.values().any(|a| a.credential == *credential) {
                    return Err(RuleViolation::AgentCredentialReused);
                }
                if self
                    .agents
                    .values()
                    .any(|a| a.revoked.is_none() && a.name == *name)
                {
                    return Err(RuleViolation::AgentNameInUse(name.clone()));
                }
                Ok(())
            }
            Event::AgentRevoked { agent, by, .. } => {
                if *by == Resolution::Reevaluated {
                    return Err(RuleViolation::RevokeNotADecision);
                }
                match self.agents.get(agent) {
                    None => Err(RuleViolation::UnknownAgent(*agent)),
                    Some(a) if a.revoked.is_some() => {
                        Err(RuleViolation::AgentAlreadyRevoked(*agent))
                    }
                    Some(_) => Ok(()),
                }
            }
            Event::ArchivePruned {
                last_seq,
                last_hash,
            } => {
                if *last_seq == 0 || *last_seq > self.last_seq {
                    return Err(RuleViolation::ArchiveBeyondLog {
                        last_seq: *last_seq,
                        log_end: self.last_seq,
                    });
                }
                if self.is_pruned(*last_seq, last_hash) {
                    return Err(RuleViolation::AlreadyPruned {
                        last_seq: *last_seq,
                    });
                }
                Ok(())
            }
        }
    }

    fn no_running_job(&self, task: TaskId) -> Result<(), RuleViolation> {
        match self.running_job_of(task) {
            Some(job) => Err(RuleViolation::TaskHasRunningJob { task, job }),
            None => Ok(()),
        }
    }

    /// [`State::apply`] for an event written at schema `version`. A human decision without the
    /// operator's signature is refused if it, or any event before it, was written at version 13 or
    /// later: the version is the writer's to choose, so an old one is no excuse once the log has
    /// moved past it (ADR-0070).
    pub fn apply_written(
        &mut self,
        seq: u64,
        at: Timestamp,
        origin: Origin,
        version: u32,
        event: &Event,
    ) -> Result<(), RuleViolation> {
        if event.signed() == Some(&Signed::Unrecorded)
            && (version >= SIGNED_SINCE || self.signed_since.is_some())
        {
            return Err(RuleViolation::HumanDecisionUnsigned);
        }
        self.apply(seq, at, origin, event)?;
        if version >= SIGNED_SINCE && self.signed_since.is_none() {
            self.signed_since = Some(seq);
        }
        Ok(())
    }

    /// Since which event human decisions must be signed, if the log has reached that version.
    pub fn signed_since(&self) -> Option<u64> {
        self.signed_since
    }

    /// Applies `event`, which was appended at `seq` at time `at`. A refused event changes nothing.
    fn apply(
        &mut self,
        seq: u64,
        at: Timestamp,
        origin: Origin,
        event: &Event,
    ) -> Result<(), RuleViolation> {
        if seq <= self.last_seq {
            return Err(RuleViolation::OutOfOrder {
                seq,
                last: self.last_seq,
            });
        }
        self.check_stamped(at, origin, event)?;
        match event {
            Event::TaskCreated { request } => {
                self.tasks.insert(
                    TaskId::from_seq(seq),
                    Task {
                        created_at: at,
                        request: *request,
                        completed_at: None,
                        open_holds: BTreeSet::new(),
                        cancelled: None,
                    },
                );
            }
            Event::TaskCompleted { task } => {
                if let Some(t) = self.tasks.get_mut(task) {
                    t.completed_at = Some(at);
                }
            }
            Event::JobStarted { task, .. } => {
                self.jobs.insert(
                    JobId::from_seq(seq),
                    Job {
                        task: *task,
                        started_at: at,
                        ended: None,
                        reason: None,
                        values: None,
                    },
                );
            }
            Event::GateAnswered {
                job,
                gate,
                inputs,
                answer,
                ..
            } => {
                if *answer == Answer::Unknown
                    && let Some(task) = self.jobs.get(job).map(|j| j.task)
                {
                    self.pending = Some(Pending {
                        job: *job,
                        task,
                        gate: *gate,
                        inputs: *inputs,
                        job_ended: false,
                    });
                }
            }
            Event::JobEnded {
                job,
                outcome,
                reason,
            } => {
                if let Some(j) = self.jobs.get_mut(job) {
                    j.ended = Some((at, *outcome));
                    j.reason = Some(reason.clone());
                }
                if let Some(p) = self.pending.as_mut() {
                    p.job_ended = true;
                }
            }
            Event::HoldOpened {
                task,
                gate,
                inputs,
                reason,
                stakes,
            } => {
                let id = HoldId::from_seq(seq);
                self.holds.insert(
                    id,
                    Hold {
                        task: *task,
                        gate: *gate,
                        inputs: *inputs,
                        reason: *reason,
                        stakes: *stakes,
                        opened_at: at,
                        resolved: None,
                    },
                );
                if let Some(t) = self.tasks.get_mut(task) {
                    t.open_holds.insert(id);
                }
                // This is the hold an unknown answer was waiting for, if there was one.
                self.pending = None;
            }
            Event::HoldResolved {
                hold, by, reason, ..
            } => {
                if let Some(h) = self.holds.get_mut(hold) {
                    h.resolved = Some(Resolved {
                        at,
                        by: *by,
                        reason: reason.clone(),
                    });
                    if let Some(t) = self.tasks.get_mut(&h.task) {
                        t.open_holds.remove(hold);
                    }
                }
            }
            Event::TaskCancelled {
                task, by, reason, ..
            } => {
                if let Some(t) = self.tasks.get_mut(task) {
                    // The holds that were open stay unresolved: they were ended, not released.
                    t.open_holds.clear();
                    t.cancelled = Some(Cancelled {
                        at,
                        by: *by,
                        reason: reason.clone(),
                    });
                }
            }
            // Evidence only: it changes no state.
            Event::HoldRefused { .. } => {}
            Event::PolicyApproved {
                document,
                by,
                reason,
                ..
            } => {
                self.policy = Some(PolicyApproval {
                    seq,
                    at,
                    by: *by,
                    reason: reason.clone(),
                    document: document.clone(),
                })
            }
            Event::HandleReleased {
                task,
                handle,
                by,
                reason,
                ..
            } => self.handles.push(ReleasedHandle {
                seq,
                at,
                task: *task,
                handle: *handle,
                by: *by,
                reason: reason.clone(),
            }),
            Event::MergeApproved {
                task,
                repo,
                pr,
                head,
                method,
                by,
                reason,
                ..
            } => self.merges.push(ApprovedMerge {
                seq,
                at,
                task: *task,
                repo: repo.clone(),
                pr: *pr,
                head: head.clone(),
                method: *method,
                by: *by,
                reason: reason.clone(),
            }),
            Event::AgentIssued {
                name,
                credential,
                by,
                reason,
                ..
            } => {
                self.agents.insert(
                    AgentId::from_seq(seq),
                    Agent {
                        at,
                        name: name.clone(),
                        credential: *credential,
                        by: *by,
                        reason: reason.clone(),
                        revoked: None,
                    },
                );
            }
            Event::AgentRevoked {
                agent, by, reason, ..
            } => {
                if let Some(a) = self.agents.get_mut(agent) {
                    a.revoked = Some(Revoked {
                        at,
                        by: *by,
                        reason: reason.clone(),
                    });
                }
            }
            Event::ArchivePruned {
                last_seq,
                last_hash,
            } => self.pruned.push(Pruned {
                seq,
                at,
                last_seq: *last_seq,
                last_hash: *last_hash,
            }),
            Event::JobValues { job, values } => {
                if let Some(j) = self.jobs.get_mut(job) {
                    j.values = Some(values.clone());
                }
            }
        }
        if let Some(Signed::Operator(_)) = event.signed() {
            self.last_approval = Some(seq);
        }
        self.last_seq = seq;
        self.last_at = Some(at);
        Ok(())
    }

    /// The state as a snapshot stores it (ADR-0026). It refuses while an unknown answer is waiting for
    /// its hold, since a snapshot in the middle of that transaction could not be resumed.
    ///
    /// Every field is named on purpose: adding one to [`State`], [`Task`], [`Job`], [`Hold`] or
    /// [`Resolved`] stops compiling here until the image carries it.
    pub fn to_image(&self) -> Result<StateImage, JobId> {
        let State {
            tasks,
            jobs,
            holds,
            pruned,
            policy,
            handles,
            agents,
            merges,
            pending,
            last_approval,
            signed_since,
            last_seq,
            last_at,
        } = self;
        if let Some(pending) = pending {
            return Err(pending.job);
        }
        Ok(StateImage {
            last_seq: *last_seq,
            last_at: last_at.map(Timestamp::as_millis),
            last_approval: *last_approval,
            signed_since: *signed_since,
            merges: merges
                .iter()
                .map(|m| {
                    let ApprovedMerge {
                        seq,
                        at,
                        task,
                        repo,
                        pr,
                        head,
                        method,
                        by,
                        reason,
                    } = m;
                    MergeImage {
                        seq: *seq,
                        at: at.as_millis(),
                        task: *task,
                        repo: repo.clone(),
                        pr: *pr,
                        head: head.clone(),
                        method: *method,
                        by: *by,
                        reason: reason.clone(),
                    }
                })
                .collect(),
            tasks: tasks
                .iter()
                .map(|(id, t)| {
                    let Task {
                        created_at,
                        request,
                        completed_at,
                        open_holds,
                        cancelled,
                    } = t;
                    TaskImage {
                        id: *id,
                        created_at: created_at.as_millis(),
                        request: *request,
                        completed_at: completed_at.map(Timestamp::as_millis),
                        open_holds: open_holds.iter().copied().collect(),
                        cancelled: cancelled.as_ref().map(|c| {
                            let Cancelled { at, by, reason } = c;
                            CancelledImage {
                                at: at.as_millis(),
                                by: *by,
                                reason: reason.clone(),
                            }
                        }),
                    }
                })
                .collect(),
            jobs: jobs
                .iter()
                .map(|(id, j)| {
                    let Job {
                        task,
                        started_at,
                        ended,
                        reason,
                        values,
                    } = j;
                    JobImage {
                        id: *id,
                        task: *task,
                        started_at: started_at.as_millis(),
                        ended: ended.map(|(at, outcome)| JobEndImage {
                            at: at.as_millis(),
                            outcome,
                            reason: reason.clone().unwrap_or_else(legacy_reason),
                        }),
                        values: values.clone(),
                    }
                })
                .collect(),
            holds: holds
                .iter()
                .map(|(id, h)| {
                    let Hold {
                        task,
                        gate,
                        inputs,
                        reason,
                        stakes,
                        opened_at,
                        resolved,
                    } = h;
                    HoldImage {
                        id: *id,
                        task: *task,
                        gate: *gate,
                        inputs: *inputs,
                        reason: *reason,
                        stakes: *stakes,
                        opened_at: opened_at.as_millis(),
                        resolved: resolved.as_ref().map(|r| {
                            let Resolved { at, by, reason } = r;
                            ResolvedImage {
                                at: at.as_millis(),
                                by: *by,
                                reason: reason.clone(),
                            }
                        }),
                    }
                })
                .collect(),
            pruned: pruned
                .iter()
                .map(|p| {
                    let Pruned {
                        seq,
                        at,
                        last_seq,
                        last_hash,
                    } = p;
                    PrunedImage {
                        seq: *seq,
                        at: at.as_millis(),
                        last_seq: *last_seq,
                        last_hash: *last_hash,
                    }
                })
                .collect(),
            policy: policy.as_ref().map(|p| {
                let PolicyApproval {
                    seq,
                    at,
                    by,
                    reason,
                    document,
                } = p;
                PolicyImage {
                    seq: *seq,
                    at: at.as_millis(),
                    by: *by,
                    reason: reason.clone(),
                    document: document.clone(),
                }
            }),
            handles: handles
                .iter()
                .map(|h| {
                    let ReleasedHandle {
                        seq,
                        at,
                        task,
                        handle,
                        by,
                        reason,
                    } = h;
                    HandleImage {
                        seq: *seq,
                        at: at.as_millis(),
                        task: *task,
                        handle: *handle,
                        by: *by,
                        reason: reason.clone(),
                    }
                })
                .collect(),
            agents: agents
                .iter()
                .map(|(id, a)| {
                    let Agent {
                        at,
                        name,
                        credential,
                        by,
                        reason,
                        revoked,
                    } = a;
                    AgentImage {
                        id: *id,
                        at: at.as_millis(),
                        name: name.clone(),
                        credential: *credential,
                        by: *by,
                        reason: reason.clone(),
                        revoked: revoked.as_ref().map(|r| RevokedImage {
                            at: r.at.as_millis(),
                            by: r.by,
                            reason: r.reason.clone(),
                        }),
                    }
                })
                .collect(),
        })
    }

    /// Rebuilds the state a snapshot carried. The snapshot is signed, so this guards against a build
    /// that wrote a bad one, not against an attacker: it refuses duplicate ids and a state that breaks
    /// the invariants, rather than start from something that could never have been replayed.
    pub fn from_image(image: StateImage) -> Result<Self, String> {
        let StateImage {
            last_seq,
            last_at,
            tasks,
            jobs,
            holds,
            pruned,
            policy,
            handles,
            agents,
            last_approval,
            signed_since,
            merges,
        } = image;
        let mut state = State {
            last_approval,
            signed_since,
            merges: merges
                .into_iter()
                .map(|m| ApprovedMerge {
                    seq: m.seq,
                    at: Timestamp::from_millis(m.at),
                    task: m.task,
                    repo: m.repo,
                    pr: m.pr,
                    head: m.head,
                    method: m.method,
                    by: m.by,
                    reason: m.reason,
                })
                .collect(),
            last_seq,
            last_at: last_at.map(Timestamp::from_millis),
            ..State::default()
        };
        for t in tasks {
            let task = Task {
                created_at: Timestamp::from_millis(t.created_at),
                request: t.request,
                completed_at: t.completed_at.map(Timestamp::from_millis),
                open_holds: t.open_holds.into_iter().collect(),
                cancelled: t.cancelled.map(|c| Cancelled {
                    at: Timestamp::from_millis(c.at),
                    by: c.by,
                    reason: c.reason,
                }),
            };
            if state.tasks.insert(t.id, task).is_some() {
                return Err(format!("task {} appears twice", t.id));
            }
        }
        for j in jobs {
            let job = Job {
                task: j.task,
                started_at: Timestamp::from_millis(j.started_at),
                ended: j
                    .ended
                    .as_ref()
                    .map(|e| (Timestamp::from_millis(e.at), e.outcome)),
                reason: j.ended.as_ref().map(|e| e.reason.clone()),
                values: j.values,
            };
            if state.jobs.insert(j.id, job).is_some() {
                return Err(format!("job {} appears twice", j.id));
            }
        }
        for h in holds {
            let hold = Hold {
                task: h.task,
                gate: h.gate,
                inputs: h.inputs,
                reason: h.reason,
                stakes: h.stakes,
                opened_at: Timestamp::from_millis(h.opened_at),
                resolved: h.resolved.map(|r| Resolved {
                    at: Timestamp::from_millis(r.at),
                    by: r.by,
                    reason: r.reason,
                }),
            };
            if state.holds.insert(h.id, hold).is_some() {
                return Err(format!("hold {} appears twice", h.id));
            }
        }
        for p in pruned {
            state.pruned.push(Pruned {
                seq: p.seq,
                at: Timestamp::from_millis(p.at),
                last_seq: p.last_seq,
                last_hash: p.last_hash,
            });
        }
        state.policy = policy.map(|p| PolicyApproval {
            seq: p.seq,
            at: Timestamp::from_millis(p.at),
            by: p.by,
            reason: p.reason,
            document: p.document,
        });
        state.handles = handles
            .into_iter()
            .map(|h| ReleasedHandle {
                seq: h.seq,
                at: Timestamp::from_millis(h.at),
                task: h.task,
                handle: h.handle,
                by: h.by,
                reason: h.reason,
            })
            .collect();
        for a in agents {
            let agent = Agent {
                at: Timestamp::from_millis(a.at),
                name: a.name,
                credential: a.credential,
                by: a.by,
                reason: a.reason,
                revoked: a.revoked.map(|r| Revoked {
                    at: Timestamp::from_millis(r.at),
                    by: r.by,
                    reason: r.reason,
                }),
            };
            if state.agents.insert(a.id, agent).is_some() {
                return Err(format!("agent {} appears twice", a.id));
            }
        }
        let broken = state.invariant_violations();
        if !broken.is_empty() {
            return Err(format!(
                "the state breaks its own rules: {}",
                broken.join("; ")
            ));
        }
        Ok(state)
    }

    /// The job whose unknown answer has no hold yet, if the log ended too soon. Replay refuses to
    /// finish while this is set (ADR-0020).
    pub fn unfinished_hold(&self) -> Option<JobId> {
        self.pending.as_ref().map(|p| p.job)
    }

    /// The properties that must hold after every event, from ADR-0009 and ADR-0013. Empty means none
    /// is broken. `apply` already refuses the events that would break them, so a non-empty answer
    /// means the state was built some other way, which the tests treat as a failure.
    pub fn invariant_violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (id, task) in &self.tasks {
            let running = self.running_job_of(*id);
            if !task.open_holds.is_empty()
                && let Some(job) = running
            {
                out.push(format!(
                    "task {id} has an open hold and job {job} is running"
                ));
            }
            if task.completed_at.is_some() {
                if !task.open_holds.is_empty() {
                    out.push(format!("completed task {id} has open holds"));
                }
                if let Some(job) = running {
                    out.push(format!("completed task {id} has job {job} running"));
                }
            }
            let actual: BTreeSet<HoldId> = self
                .holds
                .iter()
                .filter(|(_, h)| h.task == *id && h.resolved.is_none() && task.cancelled.is_none())
                .map(|(hid, _)| *hid)
                .collect();
            if actual != task.open_holds {
                out.push(format!("task {id} lists the wrong open holds"));
            }
            if let Some(c) = &task.cancelled {
                if task.completed_at.is_some() {
                    out.push(format!("task {id} is both completed and cancelled"));
                }
                if let Some(job) = running {
                    out.push(format!("cancelled task {id} has job {job} running"));
                }
                // Every hold it still has was open when it was cancelled, so the decider had to be
                // allowed at each.
                for (hid, h) in &self.holds {
                    if h.task == *id && h.resolved.is_none() && !h.stakes.may_be_cancelled_by(c.by)
                    {
                        out.push(format!(
                            "task {id} was cancelled by {:?}, which may not cancel it with hold {hid} open",
                            c.by
                        ));
                    }
                }
            }
        }
        if let Some(job) = self.unfinished_hold() {
            out.push(format!("job {job}'s unknown answer has no hold"));
        }
        for (id, job) in &self.jobs {
            if !self.tasks.contains_key(&job.task) {
                out.push(format!("job {id} belongs to missing task {}", job.task));
            }
        }
        for (i, h) in self.handles.iter().enumerate() {
            if !self.tasks.contains_key(&h.task) {
                out.push(format!(
                    "handle {} belongs to missing task {}",
                    h.handle, h.task
                ));
            }
            if h.by != Resolution::Human {
                out.push(format!("handle {} was released by {:?}", h.handle, h.by));
            }
            if self.handles[..i]
                .iter()
                .any(|g| g.task == h.task && g.handle == h.handle)
            {
                out.push(format!(
                    "handle {} is released to task {} twice",
                    h.handle, h.task
                ));
            }
        }
        for (i, m) in self.merges.iter().enumerate() {
            if !self.tasks.contains_key(&m.task) {
                out.push(format!(
                    "merge {} belongs to missing task {}",
                    m.seq, m.task
                ));
            }
            if m.by != Resolution::Human {
                out.push(format!("merge {} was approved by {:?}", m.seq, m.by));
            }
            if self.merges[..i].iter().any(|n| n.task == m.task) {
                out.push(format!("task {} has two merges approved", m.task));
            }
        }
        for (id, a) in &self.agents {
            if a.by != Resolution::Human {
                out.push(format!("agent {id} was issued by {:?}", a.by));
            }
            if self
                .agents
                .range(..id)
                .any(|(_, b)| b.credential == a.credential)
            {
                out.push(format!(
                    "agent {id} shares a credential with an earlier one"
                ));
            }
            if a.revoked.is_none()
                && self
                    .agents
                    .iter()
                    .any(|(other, b)| other != id && b.revoked.is_none() && b.name == a.name)
            {
                out.push(format!(
                    "agent {id} and another credential in force are both named {}",
                    a.name
                ));
            }
            if let Some(r) = &a.revoked
                && (r.at < a.at || r.by == Resolution::Reevaluated)
            {
                out.push(format!("agent {id} has an impossible revocation"));
            }
        }
        for (i, p) in self.pruned.iter().enumerate() {
            if p.last_seq == 0 || p.last_seq > p.seq || p.seq > self.last_seq {
                out.push(format!(
                    "the prune recorded at event {} names an archive ending at event {}",
                    p.seq, p.last_seq
                ));
            }
            if self.pruned[..i]
                .iter()
                .any(|q| q.last_seq == p.last_seq && q.last_hash == p.last_hash)
            {
                out.push(format!(
                    "the archive ending at event {} is recorded as pruned twice",
                    p.last_seq
                ));
            }
        }
        for (id, hold) in &self.holds {
            if !self.tasks.contains_key(&hold.task) {
                out.push(format!("hold {id} belongs to missing task {}", hold.task));
            }
            if let Some(r) = &hold.resolved
                && !hold.stakes.may_be_resolved_by(r.by)
            {
                out.push(format!(
                    "hold {id} was released by {:?}, which may not release it",
                    r.by
                ));
            }
        }
        out
    }
}

/// The reason a job from before version 7 of the image is read with.
fn legacy_reason() -> ReasonCode {
    ReasonCode::new("legacy.unrecorded").unwrap_or_else(ReasonCode::unavailable)
}
