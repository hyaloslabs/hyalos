//! Tasks, jobs and holds, rebuilt from events (ADR-0003, ADR-0019).
//!
//! [`State`] is a pure fold over events. [`State::apply`] checks that an event is legal for the state it
//! lands on and refuses it otherwise, so an engine bug or a damaged log shows up as an error, not as
//! silently wrong state. Everything here is ordered (`BTreeMap`), so two replays of one log are equal.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use hyalos_types::snapshot::{
    HoldImage, JobEndImage, JobImage, ResolvedImage, StateImage, TaskImage,
};
use hyalos_types::{
    Answer, Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, Origin, ReasonCode,
    Resolution, Stakes, TaskId, Timestamp,
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
    pending: Option<Pending>,
    last_seq: u64,
    last_at: Option<Timestamp>,
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
            Event::HoldResolved { hold, by, .. } => match self.holds.get(hold) {
                None => Err(RuleViolation::UnknownHold(*hold)),
                Some(h) if h.resolved.is_some() => Err(RuleViolation::HoldNotOpen(*hold)),
                Some(h) if !h.stakes.may_be_resolved_by(*by) => {
                    Err(RuleViolation::ResolutionNotAuthorized {
                        hold: *hold,
                        by: *by,
                        stakes: h.stakes,
                    })
                }
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
    pub fn apply(
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
            Event::HoldResolved { hold, by, reason } => {
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
            pending,
            last_seq,
            last_at,
        } = self;
        if let Some(pending) = pending {
            return Err(pending.job);
        }
        Ok(StateImage {
            last_seq: *last_seq,
            last_at: last_at.map(Timestamp::as_millis),
            tasks: tasks
                .iter()
                .map(|(id, t)| {
                    let Task {
                        created_at,
                        request,
                        completed_at,
                        open_holds,
                    } = t;
                    TaskImage {
                        id: *id,
                        created_at: created_at.as_millis(),
                        request: *request,
                        completed_at: completed_at.map(Timestamp::as_millis),
                        open_holds: open_holds.iter().copied().collect(),
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
                    } = j;
                    JobImage {
                        id: *id,
                        task: *task,
                        started_at: started_at.as_millis(),
                        ended: ended.map(|(at, outcome)| JobEndImage {
                            at: at.as_millis(),
                            outcome,
                        }),
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
        } = image;
        let mut state = State {
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
            };
            if state.tasks.insert(t.id, task).is_some() {
                return Err(format!("task {} appears twice", t.id));
            }
        }
        for j in jobs {
            let job = Job {
                task: j.task,
                started_at: Timestamp::from_millis(j.started_at),
                ended: j.ended.map(|e| (Timestamp::from_millis(e.at), e.outcome)),
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
