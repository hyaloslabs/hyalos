//! Thousands of seeds, each under its own fault schedule, each run checked (S8.1, ADR-0013,
//! ADR-0104).

use std::collections::BTreeMap;
use std::ops::Range;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use hyalos_types::JobOutcome;

use crate::schedule::{Boundary, Profile};
use crate::{Run, Schedule, TOKEN_BUDGET, run_with};

/// The one command that replays a seed on a developer's machine.
pub fn reproduce(seed: u64) -> String {
    format!("cargo run -q --locked -p hyalos-world --bin sim -- {seed}")
}

/// A seed whose run failed `run_with`'s own checks or [`check`]'s, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub seed: u64,
    pub profile: Profile,
    pub why: String,
}

/// What a sweep ran into: how many runs had each profile and each outcome, and the failures.
#[derive(Debug, Default)]
pub struct Sweep {
    pub runs: u64,
    pub profiles: BTreeMap<String, u64>,
    pub outcomes: BTreeMap<String, u64>,
    /// Gate-outage runs that asked a gate after the outage began, which must all be held.
    pub held_by_outage: u64,
    /// How each merge job ended, so a sweep shows the merge invariant was checked on real merges
    /// and on refused ones (ADR-0108).
    pub merges: BTreeMap<String, u64>,
    pub failures: Vec<Failure>,
}

impl Sweep {
    /// The profiles and outcomes no run reached. A sweep with gaps passed without exercising what it
    /// is for, so it is not a pass.
    pub fn gaps(&self) -> Vec<String> {
        let mut gaps: Vec<String> = Profile::ALL
            .iter()
            .map(ToString::to_string)
            .filter(|p| !self.profiles.contains_key(p))
            .map(|p| format!("no {p} schedule"))
            .collect();
        for want in ["Succeeded", "Failed", "DeadlineExceeded", "Held"] {
            if !self.outcomes.contains_key(want) {
                gaps.push(format!("no run ended {want}"));
            }
        }
        if self.held_by_outage == 0 {
            gaps.push("no gate outage was ever asked a question".into());
        }
        for want in ["Merged", "HeadMoved", "Expired"] {
            if !self.merges.contains_key(want) {
                gaps.push(format!("no merge ended {want}"));
            }
        }
        gaps
    }

    fn record(&mut self, schedule: &Schedule, run: &Run) {
        self.runs += 1;
        *self
            .profiles
            .entry(schedule.profile.to_string())
            .or_default() += 1;
        *self
            .outcomes
            .entry(format!("{:?}", run.outcome))
            .or_default() += 1;
        if gate_outage_reached(schedule, run) {
            self.held_by_outage += 1;
        }
        if let Some(ending) = run.counts.merge {
            *self.merges.entry(format!("{ending:?}")).or_default() += 1;
        }
    }
}

fn gate_outage_reached(schedule: &Schedule, run: &Run) -> bool {
    schedule
        .gate
        .down_after
        .is_some_and(|after| run.counts.gates_asked > after)
}

/// The properties every run must hold under any schedule, beyond those `run_with` checks itself
/// (ADR-0009's invariants, the projection equal to a replay of the log, a log that verifies).
pub fn check(schedule: &Schedule, run: &Run) -> Result<(), String> {
    let c = &run.counts;
    let kinds: Vec<&str> = run.events.iter().map(|e| e.kind.as_str()).collect();
    let fail = |why: String| Err(why);

    // Every question to a gate is asked and answered in the log, so none is skipped on a fault.
    if c.gates_asked != c.issues_read + c.model_replies + c.egress_checks {
        return fail(format!(
            "{} gate questions for {} issues, {} replies and {} hosts",
            c.gates_asked, c.issues_read, c.model_replies, c.egress_checks
        ));
    }
    let answered = kinds.iter().filter(|k| **k == "gate.answered").count() as u32;
    if answered != c.gates_asked {
        return fail(format!(
            "{answered} gate.answered for {} questions",
            c.gates_asked
        ));
    }

    // How the job ended is what the log says, and only a succeeded job completes its task.
    match run.outcome {
        JobOutcome::Held if !kinds.contains(&"hold.opened") => {
            return fail("held with no hold.opened".into());
        }
        JobOutcome::Succeeded if kinds.last() != Some(&"task.completed") => {
            return fail("succeeded without task.completed last".into());
        }
        JobOutcome::Succeeded => {}
        _ if kinds.contains(&"task.completed") => {
            return fail(format!("{:?} and task.completed", run.outcome));
        }
        _ => {}
    }

    // An expired manifest authorizes nothing more (ADR-0004), and overspending never succeeds.
    if c.expired && run.outcome != JobOutcome::DeadlineExceeded {
        return fail(format!(
            "the manifest expired and the job {:?}",
            run.outcome
        ));
    }
    if c.tokens_spent > TOKEN_BUDGET && run.outcome == JobOutcome::Succeeded {
        return fail(format!("spent {} tokens and succeeded", c.tokens_spent));
    }

    // The sandbox holds the job's placeholder and nothing else (ADR-0016).
    for env in &run.sandbox_env {
        if env.len() != 1 || env.get("HYALOS_CREDENTIAL") != Some(&run.fixtures.job_credential) {
            return fail(format!(
                "the sandbox was started with {} variables, not the placeholder alone",
                env.len()
            ));
        }
    }

    // Fail closed (ADR-0009, ADR-0020): once the gate is down, the next question holds the task.
    if gate_outage_reached(schedule, run) && run.outcome != JobOutcome::Held {
        return fail(format!(
            "the gate was down and the job {:?} instead of held",
            run.outcome
        ));
    }

    // What each profile promises.
    match schedule.profile {
        Profile::Calm => {
            if c.forge_faults + c.model_faults + c.gates_refused > 0 {
                return fail("a fault or a refusal under a calm schedule".into());
            }
            if run.outcome == JobOutcome::Held {
                return fail("held under a calm schedule".into());
            }
            if run.outcome == JobOutcome::Failed && c.tokens_spent <= TOKEN_BUDGET {
                return fail("failed under a calm schedule within budget".into());
            }
        }
        Profile::Outage(Boundary::Forge) => {
            let after = schedule.forge.down_after.unwrap_or(0);
            if c.issues_read + c.pull_requests + c.merge_forge_served > after {
                return fail("the forge served a request after its outage began".into());
            }
        }
        Profile::Outage(Boundary::Model) => {
            let after = schedule.model.down_after.unwrap_or(0);
            if c.model_replies > after {
                return fail("the model replied after its outage began".into());
            }
        }
        Profile::Outage(Boundary::Substrate) => {
            if c.sandbox_started || run.outcome != JobOutcome::Failed {
                return fail(format!(
                    "the runtime refused every start and the job {:?}",
                    run.outcome
                ));
            }
        }
        Profile::Outage(Boundary::Gate) | Profile::Usual | Profile::Storm => {}
    }

    // Nothing planted reaches a file the run left (ADR-0036).
    match run.leaks() {
        Ok(leaks) if leaks.is_empty() => Ok(()),
        Ok(leaks) => fail(format!(
            "planted strings in the log's files: {:?}",
            leaks.iter().map(|l| l.fixture).collect::<Vec<_>>()
        )),
        Err(e) => fail(format!("the scan could not read the run: {e}")),
    }
}

/// Runs one seed under its own schedule in `dir` and checks it. A run that panics is a failure of
/// that seed, so a sweep names it rather than dying with it.
pub fn run_seed(seed: u64, dir: &Path) -> (Schedule, Result<Run, String>) {
    let schedule = Schedule::for_seed(seed);
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        run_with(seed, &schedule, dir)
            .map_err(|e| e.to_string())
            .and_then(|run| check(&schedule, &run).map(|()| run))
    }))
    .unwrap_or_else(|_| Err("the run panicked".into()));
    (schedule, result)
}

/// Runs every seed in `seeds` on `threads` threads, each in its own directory under `root`. A passing
/// run's directory is removed; a failing run's is kept for reading.
pub fn sweep(seeds: Range<u64>, root: &Path, threads: usize) -> Sweep {
    let next = AtomicU64::new(seeds.start);
    let total = Mutex::new(Sweep::default());
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, Ordering::SeqCst);
                    if seed >= seeds.end {
                        break;
                    }
                    let dir = seed_dir(root, seed);
                    let _ = std::fs::remove_dir_all(&dir);
                    let (schedule, result) = run_seed(seed, &dir);
                    // A poisoned lock still holds every count; giving up on it would skip seeds.
                    let mut total = total.lock().unwrap_or_else(PoisonError::into_inner);
                    match result {
                        Ok(run) => {
                            total.record(&schedule, &run);
                            drop(total);
                            let _ = std::fs::remove_dir_all(&dir);
                        }
                        Err(why) => {
                            total.runs += 1;
                            total.failures.push(Failure {
                                seed,
                                profile: schedule.profile,
                                why,
                            });
                        }
                    }
                }
            });
        }
    });
    let mut total = total.into_inner().unwrap_or_else(PoisonError::into_inner);
    total.failures.sort_by_key(|f| f.seed);
    total
}

/// Where a seed's run is written under `root`.
pub fn seed_dir(root: &Path, seed: u64) -> PathBuf {
    root.join(format!("seed-{seed}"))
}
