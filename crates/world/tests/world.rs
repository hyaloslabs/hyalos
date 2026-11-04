//! S1.5 and S1.1: a seed fixes a whole simulated job, and nothing planted reaches the log
//! (ADR-0003, ADR-0013, ADR-0036).

use std::collections::BTreeSet;
use std::path::PathBuf;

use hyalos_types::JobOutcome;
use hyalos_world::{Fixtures, Run, TOKEN_BUDGET, run_job, scan};

type Res = Result<(), Box<dyn std::error::Error>>;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hyalos-world-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Runs `seed` in a fresh directory.
fn run(seed: u64, name: &str) -> Result<Run, Box<dyn std::error::Error>> {
    run_job(seed, &dir(name))
}

/// Everything about the log that a replay must reproduce: each event's time, origin, kind, schema
/// version and payload bytes, and the hash that commits to all of them.
fn fingerprint(run: &Run) -> (Vec<String>, String) {
    let events = run
        .events
        .iter()
        .map(|e| {
            format!(
                "{} {} {:?} {} v{} {}",
                e.seq,
                e.at.as_millis(),
                e.origin,
                e.kind,
                e.schema_version,
                String::from_utf8_lossy(&e.payload)
            )
        })
        .collect();
    (events, run.head.to_string())
}

// ---- S1.5: the same seed, the same run -----------------------------------------------------------

#[test]
fn the_same_seed_gives_the_same_log_byte_for_byte() -> Res {
    for seed in [0, 1, 7, 42, 1_000_003] {
        let a = run(seed, &format!("same-a-{seed}"))?;
        let b = run(seed, &format!("same-b-{seed}"))?;
        assert_eq!(
            fingerprint(&a),
            fingerprint(&b),
            "seed {seed} did not replay"
        );
        assert_eq!(a.counts, b.counts, "seed {seed}");
        assert_eq!(a.outcome, b.outcome, "seed {seed}");
    }
    Ok(())
}

#[test]
fn different_seeds_give_different_logs() -> Res {
    let heads: BTreeSet<String> = (0..20)
        .map(|seed| run(seed, &format!("diff-{seed}")).map(|r| r.head.to_string()))
        .collect::<Result<_, _>>()?;
    assert_eq!(heads.len(), 20);
    Ok(())
}

#[test]
fn a_simulated_job_runs_end_to_end_through_the_real_engine() -> Res {
    // Every run is checked inside `run_job` (invariants, replay equals state, log verifies), so a
    // run that returns has passed. Over many seeds, every path of the job lifecycle must occur.
    let mut outcomes = BTreeSet::new();
    let mut total = hyalos_world::Counts::default();
    let mut kinds = BTreeSet::new();
    for seed in 0..150 {
        let r = run(seed, &format!("many-{}", seed % 4))?;
        outcomes.insert(format!("{:?}", r.outcome));
        total.issues_read += r.counts.issues_read;
        total.forge_faults += r.counts.forge_faults;
        total.model_calls += r.counts.model_calls;
        total.model_faults += r.counts.model_faults;
        total.model_prompts_with_forge_content += r.counts.model_prompts_with_forge_content;
        total.pull_requests += r.counts.pull_requests;
        total.gates_refused += r.counts.gates_refused;
        total.sandbox_started |= r.counts.sandbox_started;
        total.expired |= r.counts.expired;
        if r.counts.expired {
            assert_eq!(r.outcome, JobOutcome::DeadlineExceeded, "seed {seed}");
        }
        for e in &r.events {
            kinds.insert(e.kind.clone());
        }
    }
    for want in ["Succeeded", "Failed", "DeadlineExceeded", "Held"] {
        assert!(outcomes.contains(want), "no run ended {want}: {outcomes:?}");
    }
    assert!(total.issues_read > 0 && total.model_calls > 0 && total.pull_requests > 0);
    assert!(total.forge_faults > 0, "the forge never failed");
    assert!(total.model_faults > 0, "the model never failed");
    assert!(total.gates_refused > 0, "no gate ever refused");
    assert!(total.sandbox_started);
    assert!(
        total.expired,
        "no manifest ever expired, so the broker was never asked to refuse one"
    );
    assert!(
        total.model_prompts_with_forge_content > 0,
        "forge content never reached the model, so the scan below would prove nothing"
    );
    for kind in [
        "policy.approved",
        "task.created",
        "job.started",
        "gate.answered",
        "hold.opened",
        "job.ended",
        "task.completed",
    ] {
        assert!(kinds.contains(kind), "no run wrote {kind}: {kinds:?}");
    }
    Ok(())
}

#[test]
fn every_issue_reply_and_host_goes_to_a_gate_and_a_sandbox_that_ran_out_of_time_is_a_deadline()
-> Res {
    let mut sandbox_deadline = 0;
    for seed in 0..150 {
        let r = run(seed, "gates")?;
        let c = &r.counts;
        // Content read from the forge, text from the model and a host each put a question to a gate,
        // and every question is answered in the log.
        assert_eq!(
            c.gates_asked,
            c.issues_read + c.model_replies + c.egress_checks,
            "seed {seed}"
        );
        let answered = r
            .events
            .iter()
            .filter(|e| e.kind == "gate.answered")
            .count();
        assert_eq!(answered as u32, c.gates_asked, "seed {seed}");
        if r.outcome == JobOutcome::DeadlineExceeded && !c.expired {
            sandbox_deadline += 1;
        }
    }
    assert!(sandbox_deadline > 0, "no sandbox ever ran out of time");
    Ok(())
}

#[test]
fn a_held_job_leaves_its_task_held_and_a_succeeded_job_completes_it() -> Res {
    let (mut held, mut done) = (0, 0);
    for seed in 0..150 {
        let r = run(seed, "lifecycle")?;
        let kinds: Vec<&str> = r.events.iter().map(|e| e.kind.as_str()).collect();
        match r.outcome {
            JobOutcome::Held => {
                held += 1;
                assert!(kinds.contains(&"hold.opened"));
                assert!(!kinds.contains(&"task.completed"), "seed {seed}");
            }
            JobOutcome::Succeeded => {
                done += 1;
                assert_eq!(kinds.last(), Some(&"task.completed"), "seed {seed}");
            }
            _ => assert!(!kinds.contains(&"task.completed"), "seed {seed}"),
        }
    }
    assert!(held > 0 && done > 0);
    Ok(())
}

#[test]
fn a_job_that_overspends_its_budget_does_not_succeed_and_the_sandbox_holds_only_its_placeholder()
-> Res {
    let (mut over, mut started) = (0, 0);
    for seed in 0..150 {
        let r = run(seed, "budget")?;
        if r.counts.tokens_spent > TOKEN_BUDGET {
            over += 1;
            assert_ne!(
                r.outcome,
                JobOutcome::Succeeded,
                "seed {seed} overspent and succeeded"
            );
        }
        // The sandbox is started with the credential issued for this job and no other variable:
        // not the forge token, not the provider key.
        for env in &r.sandbox_env {
            started += 1;
            assert_eq!(env.len(), 1, "seed {seed}: {env:?}");
            assert_eq!(
                env.get("HYALOS_CREDENTIAL"),
                Some(&r.fixtures.job_credential),
                "seed {seed}"
            );
        }
        assert_eq!(r.sandbox_env.len(), usize::from(r.counts.sandbox_started));
    }
    assert!(
        over > 0,
        "no run overspent, so the budget was never enforced"
    );
    assert!(started > 100);
    Ok(())
}

// ---- S1.1: nothing planted reaches the log --------------------------------------------------------

#[test]
fn a_full_job_run_leaves_no_secret_or_raw_content_in_the_log() -> Res {
    let mut with_content = 0;
    for seed in 0..150 {
        let r = run(seed, &format!("scan-{}", seed % 4))?;
        // The fixtures were in play: this run's forge content reached the model when it called it.
        if r.counts.model_prompts_with_forge_content > 0 {
            with_content += 1;
        }
        let leaks = r.leaks()?;
        assert!(leaks.is_empty(), "seed {seed} leaked {leaks:?}");
    }
    assert!(
        with_content > 10,
        "too few runs put forge content in front of the model"
    );
    Ok(())
}

#[test]
fn the_scan_finds_a_planted_string_in_a_file_that_holds_one() -> Res {
    let r = run(5, "control")?;
    assert!(r.leaks()?.is_empty());
    for (name, value) in r.fixtures.all() {
        let d = dir(&format!("control-{}", name.replace(' ', "-")));
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join("events.db"), format!("header {value} trailer"))?;
        let found = scan(&d, &r.fixtures)?;
        assert_eq!(found.len(), 1, "{name}");
        assert_eq!(found[0].fixture, name);
    }
    Ok(())
}

#[test]
fn the_scan_reads_every_file_beside_the_database() -> Res {
    let fixtures = Fixtures::for_seed(9);
    let d = dir("beside");
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("events.db"), "clean")?;
    std::fs::write(d.join("events.db-wal"), &fixtures.provider_key)?;
    let found = scan(&d, &fixtures)?;
    assert_eq!(found.len(), 1);
    assert!(found[0].file.ends_with("events.db-wal"));
    assert_eq!(found[0].fixture, "provider key");
    Ok(())
}

#[test]
fn the_fixtures_are_unique_to_the_seed_and_none_contains_another() {
    let (a, b) = (Fixtures::for_seed(1), Fixtures::for_seed(2));
    assert_ne!(a, b);
    let all = a.all();
    for (i, (_, x)) in all.iter().enumerate() {
        for (j, (_, y)) in all.iter().enumerate() {
            if i != j {
                assert!(!x.contains(y), "{x} contains {y}");
            }
            assert!(
                !b.all()[j].1.contains(x),
                "{x} is in another seed's {}",
                b.all()[j].1
            );
        }
    }
}

#[test]
fn the_job_records_the_substrate_that_ran_it() -> Res {
    // ADR-0011, ADR-0043: each job.started names its substrate, at the current schema version.
    let run = run(17, "substrate")?;
    let started: Vec<_> = run
        .events
        .iter()
        .filter(|e| e.kind == "job.started")
        .collect();
    assert_eq!(started.len(), 1);
    let event = hyalos_types::Event::decode(
        &started[0].kind,
        started[0].schema_version,
        &started[0].payload,
    )?;
    let hyalos_types::Event::JobStarted { substrate, .. } = event else {
        return Err("not a job.started".into());
    };
    assert_eq!(substrate.as_str(), "seeded");
    assert_eq!(
        started[0].schema_version,
        hyalos_types::event::SCHEMA_VERSION
    );
    Ok(())
}
