//! Tasks, jobs and holds, rebuilt from events (ADR-0003, ADR-0019).
//!
//! [`State`] is a pure fold over events. [`State::apply`] checks that an event is legal for the state it
//! lands on and refuses it otherwise, so an engine bug or a damaged log shows up as an error, not as
//! silently wrong state. Everything here is ordered (`BTreeMap`), so two replays of one log are equal.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use hyalos_types::{
    Answer, Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, Resolution, TaskId,
    Timestamp,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Open,
    /// At least one hold is open (ADR-0009).
    Held,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub created_at: Timestamp,
    pub request: Digest,
    pub completed_at: Option<Timestamp>,
    pub open_holds: BTreeSet<HoldId>,
}

impl Task {
    pub fn status(&self) -> TaskStatus {
        if self.completed_at.is_some() {
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
    pub opened_at: Timestamp,
    pub resolved: Option<(Timestamp, Resolution)>,
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
    /// The log ends after an unknown answer, before its hold. Nothing may proceed on unknown.
    UnfinishedHold(JobId),
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
            RuleViolation::OutOfOrder { seq, last } => {
                write!(f, "event {seq} arrived after event {last}")
            }
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
    pending: Option<Pending>,
    last_seq: u64,
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

    /// The sequence number of the last event applied, or 0 for none.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
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

    /// Whether `event` is legal now. It changes nothing.
    pub fn check(&self, event: &Event) -> Result<(), RuleViolation> {
        if let Some(p) = &self.pending {
            let expected = match (p.job_ended, event) {
                (
                    false,
                    Event::JobEnded {
                        job,
                        outcome: JobOutcome::Held,
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
        self.check_rules(event)
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
            Event::JobStarted { task } => {
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
            Event::HoldOpened { task, .. } => {
                if self.task(*task)?.status() == TaskStatus::Completed {
                    return Err(RuleViolation::TaskCompleted(*task));
                }
                self.no_running_job(*task)
            }
            Event::HoldResolved { hold, .. } => match self.holds.get(hold) {
                None => Err(RuleViolation::UnknownHold(*hold)),
                Some(h) if h.resolved.is_some() => Err(RuleViolation::HoldNotOpen(*hold)),
                Some(_) => Ok(()),
            },
        }
    }

    fn no_running_job(&self, task: TaskId) -> Result<(), RuleViolation> {
        match self.running_job_of(task) {
            Some(job) => Err(RuleViolation::TaskHasRunningJob { task, job }),
            None => Ok(()),
        }
    }

    /// Applies `event`, which was appended at `seq` at time `at`. A refused event changes nothing.
    pub fn apply(&mut self, seq: u64, at: Timestamp, event: &Event) -> Result<(), RuleViolation> {
        if seq <= self.last_seq {
            return Err(RuleViolation::OutOfOrder {
                seq,
                last: self.last_seq,
            });
        }
        self.check(event)?;
        match event {
            Event::TaskCreated { request } => {
                self.tasks.insert(
                    TaskId::from_seq(seq),
                    Task {
                        created_at: at,
                        request: *request,
                        completed_at: None,
                        open_holds: BTreeSet::new(),
                    },
                );
            }
            Event::TaskCompleted { task } => {
                if let Some(t) = self.tasks.get_mut(task) {
                    t.completed_at = Some(at);
                }
            }
            Event::JobStarted { task } => {
                self.jobs.insert(
                    JobId::from_seq(seq),
                    Job {
                        task: *task,
                        started_at: at,
                        ended: None,
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
            Event::JobEnded { job, outcome } => {
                if let Some(j) = self.jobs.get_mut(job) {
                    j.ended = Some((at, *outcome));
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
            } => {
                let id = HoldId::from_seq(seq);
                self.holds.insert(
                    id,
                    Hold {
                        task: *task,
                        gate: *gate,
                        inputs: *inputs,
                        reason: *reason,
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
            Event::HoldResolved { hold, by } => {
                if let Some(h) = self.holds.get_mut(hold) {
                    h.resolved = Some((at, *by));
                    if let Some(t) = self.tasks.get_mut(&h.task) {
                        t.open_holds.remove(hold);
                    }
                }
            }
        }
        self.last_seq = seq;
        Ok(())
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
                .filter(|(_, h)| h.task == *id && h.resolved.is_none())
                .map(|(hid, _)| *hid)
                .collect();
            if actual != task.open_holds {
                out.push(format!("task {id} lists the wrong open holds"));
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
        for (id, hold) in &self.holds {
            if !self.tasks.contains_key(&hold.task) {
                out.push(format!("hold {id} belongs to missing task {}", hold.task));
            }
        }
        out
    }
}
