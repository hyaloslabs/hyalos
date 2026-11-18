//! The invariants of the job state machine's TLA+ spec (`spec/Job.tla`), checked on every simulated
//! run (S8.2, ADR-0013, ADR-0108).
//!
//! Each property here has the name of the spec's invariant it stands for, so the spec and the code
//! cannot drift apart unnoticed: a change to one names the other.
//!
//! - [`HELD_TASK_RUNS_NO_JOB`]: after every event in the log, no task with an open hold has a running
//!   job. Ending the job and opening the hold are one write, with the job ended first, so this holds
//!   between events too, not only at the end.
//! - [`MERGE_ONLY_AT_APPROVED_HEAD`]: every merge the forge made was of the commit a `merge.approved`
//!   earlier in the log named for that pull request, while it was still the head, by a job that ran
//!   on that approval's task.
//! - [`EXPIRED_MANIFEST_AUTHORIZES_NOTHING`]: the broker admitted no step after its manifest's
//!   deadline.
//! - [`PROJECTION_IS_REPLAY`]: the engine's state equals a replay of the log.

use std::collections::{BTreeMap, BTreeSet};

use hyalos_engine::replay;
use hyalos_types::{Event, HoldId, JobId, TaskId, Timestamp};

use crate::Run;

pub const HELD_TASK_RUNS_NO_JOB: &str = "HeldTaskRunsNoJob";
pub const MERGE_ONLY_AT_APPROVED_HEAD: &str = "MergeOnlyAtApprovedHead";
pub const EXPIRED_MANIFEST_AUTHORIZES_NOTHING: &str = "ExpiredManifestAuthorizesNothing";
pub const PROJECTION_IS_REPLAY: &str = "ProjectionIsReplay";

/// Every invariant, by the spec's name.
pub const INVARIANTS: [&str; 4] = [
    HELD_TASK_RUNS_NO_JOB,
    MERGE_ONLY_AT_APPROVED_HEAD,
    EXPIRED_MANIFEST_AUTHORIZES_NOTHING,
    PROJECTION_IS_REPLAY,
];

/// One step the broker admitted: the job, when, and its manifest's deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    pub job: JobId,
    pub at: Timestamp,
    pub deadline: Timestamp,
}

/// A broken invariant: the spec's name for it, and what broke it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub invariant: &'static str,
    pub what: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.invariant, self.what)
    }
}

/// The spec's invariants that `run` breaks. Empty for a run that holds them all.
pub fn violations(run: &Run) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut broke = |invariant: &'static str, what: String| {
        out.push(Violation { invariant, what });
    };

    let mut events = Vec::with_capacity(run.events.len());
    for stored in &run.events {
        match Event::decode(&stored.kind, stored.schema_version, &stored.payload) {
            Ok(event) => events.push((stored.seq, event)),
            Err(e) => broke(
                PROJECTION_IS_REPLAY,
                format!("event {} cannot be read: {e}", stored.seq),
            ),
        }
    }

    // HeldTaskRunsNoJob, after every event.
    let mut task_of: BTreeMap<JobId, TaskId> = BTreeMap::new();
    let mut running: BTreeSet<JobId> = BTreeSet::new();
    let mut open: BTreeMap<HoldId, TaskId> = BTreeMap::new();
    // Each task's merge approval: the pull request and the commit.
    let mut approved: BTreeMap<TaskId, (u64, String)> = BTreeMap::new();
    for (seq, event) in &events {
        match event {
            Event::JobStarted { task, .. } => {
                let job = JobId::from_seq(*seq);
                task_of.insert(job, *task);
                running.insert(job);
            }
            Event::JobEnded { job, .. } => {
                running.remove(job);
            }
            Event::HoldOpened { task, .. } => {
                open.insert(HoldId::from_seq(*seq), *task);
            }
            Event::HoldResolved { hold, .. } => {
                open.remove(hold);
            }
            Event::TaskCancelled { task, .. } => open.retain(|_, t| t != task),
            Event::MergeApproved { task, pr, head, .. } => {
                approved.insert(*task, (*pr, head.as_str().to_string()));
            }
            _ => {}
        }
        for job in &running {
            let Some(task) = task_of.get(job) else {
                continue;
            };
            if open.values().any(|t| t == task) {
                broke(
                    HELD_TASK_RUNS_NO_JOB,
                    format!("after event {seq}, task {task} has an open hold and job {job} runs"),
                );
            }
        }
    }

    // MergeOnlyAtApprovedHead: the forge merged only what an approval named, at its head.
    for (i, merge) in run.merges.iter().enumerate() {
        let approval = approved
            .iter()
            .find(|(_, (pr, sha))| *pr == merge.pr && *sha == merge.sha);
        match approval {
            None => broke(
                MERGE_ONLY_AT_APPROVED_HEAD,
                format!(
                    "merge {i} of pull request {} has no approval of that commit",
                    merge.pr
                ),
            ),
            Some((task, _)) => {
                if merge.head != merge.sha {
                    broke(
                        MERGE_ONLY_AT_APPROVED_HEAD,
                        format!(
                            "merge {i} of pull request {} was made after its head moved",
                            merge.pr
                        ),
                    );
                }
                if !task_of.values().any(|t| t == task) {
                    broke(
                        MERGE_ONLY_AT_APPROVED_HEAD,
                        format!("merge {i} ran on no job of approved task {task}"),
                    );
                }
            }
        }
    }

    // ExpiredManifestAuthorizesNothing: at the deadline itself the manifest is in date.
    for a in &run.admissions {
        if a.at > a.deadline {
            broke(
                EXPIRED_MANIFEST_AUTHORIZES_NOTHING,
                format!(
                    "job {} was admitted at {} after its deadline {}",
                    a.job,
                    a.at.as_millis(),
                    a.deadline.as_millis()
                ),
            );
        }
    }

    // ProjectionIsReplay.
    match replay(&run.events) {
        Ok(state) if state == run.state => {}
        Ok(_) => broke(
            PROJECTION_IS_REPLAY,
            "the projection is not a replay of the log".into(),
        ),
        Err(e) => broke(
            PROJECTION_IS_REPLAY,
            format!("the log does not replay: {e}"),
        ),
    }
    out
}
