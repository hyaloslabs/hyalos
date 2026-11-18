//! S8.2: the invariants of the job state machine's TLA+ spec are checked on every simulated run,
//! under the spec's names, and the check refuses a run that breaks each one (ADR-0013, ADR-0108).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use hyalos_types::JobOutcome;
use hyalos_world::spec::{self, Admitted};
use hyalos_world::{MergeEnding, Run, run_seed};

type Res = Result<(), Box<dyn std::error::Error>>;

fn dir(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("hyalos-spec-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The first seed, under its own schedule, whose run passes and satisfies `want`.
fn first(name: &str, want: impl Fn(&Run) -> bool) -> Result<Run, Box<dyn std::error::Error>> {
    for seed in 0..400 {
        let (_, result) = run_seed(seed, &dir(&format!("{name}-{seed}")));
        if let Ok(run) = result
            && want(&run)
        {
            return Ok(run);
        }
    }
    Err(format!("no seed under 400 gives a run for {name}").into())
}

fn broken(run: &Run) -> Vec<&'static str> {
    spec::violations(run).iter().map(|v| v.invariant).collect()
}

fn spec_file(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec")
        .join(name);
    Ok(std::fs::read_to_string(path)?)
}

#[test]
fn every_invariant_here_is_one_the_spec_defines_and_the_model_checks() -> Res {
    let tla = spec_file("Job.tla")?;
    let cfg = spec_file("Job.cfg")?;
    let checked: Vec<&str> = cfg
        .split("INVARIANTS")
        .nth(1)
        .ok_or("Job.cfg checks no invariant")?
        .split_whitespace()
        .collect();
    for name in spec::INVARIANTS {
        assert!(
            tla.contains(&format!("\n{name} ==")),
            "Job.tla does not define {name}"
        );
        assert!(checked.contains(&name), "Job.cfg does not check {name}");
    }
    Ok(())
}

#[test]
fn a_run_that_holds_the_invariants_breaks_none() -> Res {
    let run = first("clean", |r| r.counts.merge == Some(MergeEnding::Merged))?;
    assert_eq!(broken(&run), Vec::<&str>::new());
    assert_eq!(run.merges.len(), 1);
    Ok(())
}

#[test]
fn a_hold_opened_before_its_job_ended_is_a_held_task_running_a_job() -> Res {
    let mut run = first("held", |r| r.outcome == JobOutcome::Held)?;
    let ended = run
        .events
        .iter()
        .position(|e| e.kind == "job.ended")
        .ok_or("no job.ended")?;
    assert_eq!(
        run.events[ended + 1].kind,
        "hold.opened",
        "one write, job first"
    );
    run.events.swap(ended, ended + 1);
    assert!(broken(&run).contains(&spec::HELD_TASK_RUNS_NO_JOB));
    Ok(())
}

#[test]
fn a_merge_of_a_commit_no_one_approved_or_after_the_head_moved_is_refused() -> Res {
    let run = first("merged", |r| r.counts.merge == Some(MergeEnding::Merged))?;
    let good = run.merges[0].clone();

    let mut other = run;
    other.merges[0].sha = "f".repeat(40);
    other.merges[0].head = "f".repeat(40);
    assert_eq!(broken(&other), vec![spec::MERGE_ONLY_AT_APPROVED_HEAD]);

    // The approved commit, merged when the head was another.
    other.merges[0] = good.clone();
    other.merges[0].head = "e".repeat(40);
    assert_eq!(broken(&other), vec![spec::MERGE_ONLY_AT_APPROVED_HEAD]);

    // A merge of another pull request at the same commit.
    other.merges[0] = good;
    other.merges[0].pr += 1;
    assert_eq!(broken(&other), vec![spec::MERGE_ONLY_AT_APPROVED_HEAD]);
    Ok(())
}

#[test]
fn a_moved_head_or_an_expired_merge_merges_nothing() -> Res {
    for ending in [MergeEnding::HeadMoved, MergeEnding::Expired] {
        let run = first(&format!("{ending:?}"), |r| r.counts.merge == Some(ending))?;
        assert!(run.merges.is_empty(), "{ending:?}");
        assert_eq!(broken(&run), Vec::<&str>::new(), "{ending:?}");
    }
    Ok(())
}

#[test]
fn a_step_admitted_after_the_deadline_is_work_an_expired_manifest_authorized() -> Res {
    let mut run = first("expired", |r| r.outcome == JobOutcome::DeadlineExceeded)?;
    let last = *run.admissions.last().ok_or("no admission")?;
    // At the deadline itself the manifest is still in date.
    run.admissions.push(Admitted {
        at: last.deadline,
        ..last
    });
    assert_eq!(broken(&run), Vec::<&str>::new());
    run.admissions.push(Admitted {
        at: hyalos_types::Timestamp::from_millis(last.deadline.as_millis() + 1),
        ..last
    });
    assert_eq!(
        broken(&run),
        vec![spec::EXPIRED_MANIFEST_AUTHORIZES_NOTHING]
    );
    Ok(())
}

#[test]
fn a_projection_that_is_not_a_replay_of_the_log_is_refused() -> Res {
    let mut run = first("replay", |r| r.outcome == JobOutcome::Succeeded)?;
    assert_eq!(
        run.events.last().map(|e| e.kind.as_str()),
        Some("task.completed")
    );
    run.events.pop();
    assert_eq!(broken(&run), vec![spec::PROJECTION_IS_REPLAY]);
    Ok(())
}
