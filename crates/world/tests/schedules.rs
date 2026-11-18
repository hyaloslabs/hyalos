//! S8.1: thousands of fault schedules, each run checked, and a failing seed replayed with one command
//! (ADR-0013, ADR-0104). CI sweeps 10,000 seeds with the `sim` binary; this sweeps fewer, or
//! `HYALOS_SIM_SEEDS` of them.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use hyalos_types::JobOutcome;
use hyalos_world::{
    Boundary, Profile, Run, Schedule, check, reproduce, run_job, run_seed, run_with, sweep,
};

type Res = Result<(), Box<dyn std::error::Error>>;

/// A fresh directory no other test in this process uses: tests run in parallel.
fn dir(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("hyalos-sched-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn fingerprint(run: &Run) -> Vec<String> {
    run.events
        .iter()
        .map(|e| {
            format!(
                "{} {} {:?} {} {}",
                e.seq,
                e.at.as_millis(),
                e.origin,
                e.kind,
                String::from_utf8_lossy(&e.payload)
            )
        })
        .collect()
}

/// The first seed whose schedule has `profile`.
fn seed_with(profile: Profile) -> u64 {
    (0..)
        .find(|s| Schedule::for_seed(*s).profile == profile)
        .unwrap_or(0)
}

#[test]
fn a_sweep_of_many_schedules_has_no_failing_seed_and_no_gap() {
    let seeds = std::env::var("HYALOS_SIM_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(400);
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let result = sweep(0..seeds, &dir("sweep"), threads);
    let failing: Vec<String> = result
        .failures
        .iter()
        .map(|f| format!("seed {}: {} ({})", f.seed, f.why, reproduce(f.seed)))
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
    assert_eq!(result.runs, seeds);
    assert_eq!(result.gaps(), Vec::<String>::new());
}

#[test]
fn a_seed_fixes_its_schedule_and_its_whole_run() -> Res {
    for profile in Profile::ALL {
        let seed = seed_with(profile);
        let (sa, a) = run_seed(seed, &dir(&format!("same-a-{seed}")));
        let (sb, b) = run_seed(seed, &dir(&format!("same-b-{seed}")));
        let (a, b) = (a?, b?);
        assert_eq!(sa, sb);
        assert_eq!(fingerprint(&a), fingerprint(&b), "seed {seed} ({profile})");
        assert_eq!(a.head.to_string(), b.head.to_string());
    }
    Ok(())
}

#[test]
fn the_usual_schedule_is_the_run_without_one() -> Res {
    // Adding schedules changed no seed's run at the fixed rates of ADR-0036.
    for seed in [0, 3, 42] {
        let plain = run_job(seed, &dir(&format!("plain-{seed}")))?;
        let usual = run_with(seed, &Schedule::default(), &dir(&format!("usual-{seed}")))?;
        assert_eq!(fingerprint(&plain), fingerprint(&usual), "seed {seed}");
    }
    Ok(())
}

#[test]
fn a_gate_that_is_down_from_the_start_holds_the_first_question() -> Res {
    let mut schedule = Schedule::calm();
    schedule.profile = Profile::Outage(Boundary::Gate);
    schedule.gate.down_after = Some(0);
    let mut held = 0;
    for seed in 0..40 {
        let run = run_with(seed, &schedule, &dir("gate-down"))?;
        check(&schedule, &run).map_err(|e| format!("seed {seed}: {e}"))?;
        if run.counts.gates_asked > 0 {
            held += 1;
            assert_eq!(run.outcome, JobOutcome::Held, "seed {seed}");
            assert_eq!(
                run.counts.gates_asked, 1,
                "seed {seed}: asked again after a hold"
            );
        }
    }
    assert!(held > 10, "too few runs reached a gate: {held}");
    Ok(())
}

#[test]
fn under_a_calm_schedule_nothing_fails_that_the_job_did_not_cause() -> Res {
    let schedule = Schedule::calm();
    let mut succeeded = 0;
    for seed in 0..40 {
        let run = run_with(seed, &schedule, &dir("calm"))?;
        check(&schedule, &run).map_err(|e| format!("seed {seed}: {e}"))?;
        succeeded += u32::from(run.outcome == JobOutcome::Succeeded);
    }
    assert!(succeeded > 10, "{succeeded}");
    Ok(())
}

// ---- The check is not vacuous: it refuses runs that break each property ---------------------------

fn passing(profile: Profile) -> Result<(Schedule, Run), Box<dyn std::error::Error>> {
    for seed in 0.. {
        let schedule = Schedule::for_seed(seed);
        if schedule.profile != profile {
            continue;
        }
        let run = run_with(seed, &schedule, &dir(&format!("doctored-{seed}")))?;
        if run.outcome == JobOutcome::Succeeded || profile != Profile::Usual {
            check(&schedule, &run)?;
            return Ok((schedule, run));
        }
    }
    Err("no seed".into())
}

#[test]
fn the_check_refuses_a_run_that_breaks_a_property() -> Res {
    let (schedule, run) = passing(Profile::Usual)?;
    assert_eq!(run.outcome, JobOutcome::Succeeded);

    let mut r = passing(Profile::Usual)?.1;
    r.events.retain(|e| e.kind != "gate.answered");
    assert!(check(&schedule, &r).is_err() || r.counts.gates_asked == 0);

    let mut r = passing(Profile::Usual)?.1;
    r.counts.expired = true;
    assert!(check(&schedule, &r).unwrap_err().contains("expired"));

    let mut r = passing(Profile::Usual)?.1;
    r.counts.tokens_spent = hyalos_world::TOKEN_BUDGET + 1;
    assert!(check(&schedule, &r).unwrap_err().contains("tokens"));

    let mut r = passing(Profile::Usual)?.1;
    r.sandbox_env[0].insert("PROVIDER_KEY".into(), r.fixtures.provider_key.clone());
    assert!(check(&schedule, &r).unwrap_err().contains("sandbox"));

    let mut r = passing(Profile::Usual)?.1;
    r.outcome = JobOutcome::Held;
    assert!(check(&schedule, &r).is_err());

    // A planted string in the run's files.
    let r = passing(Profile::Usual)?.1;
    std::fs::write(r.dir.join("stray"), &r.fixtures.forge_token)?;
    assert!(check(&schedule, &r).unwrap_err().contains("planted"));
    Ok(())
}

#[test]
fn the_check_refuses_a_run_that_breaks_its_profile() -> Res {
    let (calm, mut r) = passing(Profile::Calm)?;
    r.counts.model_faults = 1;
    assert!(check(&calm, &r).unwrap_err().contains("calm"));

    let (down, mut r) = passing(Profile::Outage(Boundary::Substrate))?;
    r.counts.sandbox_started = true;
    assert!(
        check(&down, &r)
            .unwrap_err()
            .contains("refused every start")
    );

    let (mut gate, r) = passing(Profile::Usual)?;
    gate.gate.down_after = Some(0);
    if r.counts.gates_asked > 0 && r.outcome != JobOutcome::Held {
        assert!(check(&gate, &r).unwrap_err().contains("gate was down"));
    }
    Ok(())
}

// ---- One command reproduces a seed --------------------------------------------------------------

#[test]
fn one_command_replays_a_seed_and_prints_its_schedule() {
    assert_eq!(
        reproduce(17),
        "cargo run -q --locked -p hyalos-world --bin sim -- 17"
    );
    let seed = seed_with(Profile::Storm);
    let out = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg(seed.to_string())
        .output()
        .expect("sim runs");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains(&format!("seed:      {seed}")), "{text}");
    assert!(text.contains("profile:   storm"), "{text}");
    assert!(text.contains("result:    pass"), "{text}");
    assert!(text.contains("job.started"), "{text}");
    // The same command prints the same thing.
    let again = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg(seed.to_string())
        .output()
        .expect("sim runs");
    assert_eq!(out.stdout, again.stdout);
}

#[test]
fn the_sweep_command_reports_its_runs_and_refuses_bad_arguments() {
    let out = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["--sweep", "0", "60", "2"])
        .output()
        .expect("sim runs");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("60 seeds (0..60) on 2 threads"), "{text}");
    for bad in [&["--sweep", "x", "2"][..], &["nine"], &[]] {
        let out = Command::new(env!("CARGO_BIN_EXE_sim"))
            .args(bad)
            .output()
            .expect("sim runs");
        assert_eq!(out.status.code(), Some(2), "{bad:?}");
    }
}
