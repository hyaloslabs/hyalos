//! Runs simulated jobs under fault schedules (S8.1, ADR-0104).
//!
//! ```text
//! sim <seed>                          replay one seed, print its schedule and run, keep its log
//! sim --sweep <from> <to> [threads]   run seeds from..to, fail on any failing seed or gap
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use hyalos_world::{reproduce, run_seed, seed_dir, sweep};

const USAGE: &str = "usage: sim <seed> | sim --sweep <from> <to> [threads]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // One seed's log stays at a path that names it; a sweep has a directory of its own, so two
    // sweeps, or a sweep and a replay, never share one.
    let root = std::env::temp_dir().join("hyalos-sim");
    let sweep_root = root.join(format!("sweep-{}", std::process::id()));
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--sweep", from, to] => run_sweep(from, to, None, sweep_root),
        ["--sweep", from, to, threads] => run_sweep(from, to, Some(threads), sweep_root),
        [seed] => match seed.parse() {
            Ok(seed) => one(seed, root),
            Err(_) => usage(),
        },
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn one(seed: u64, root: PathBuf) -> ExitCode {
    let dir = seed_dir(&root, seed);
    let _ = std::fs::remove_dir_all(&dir);
    let (schedule, result) = run_seed(seed, &dir);
    println!("seed:      {seed}\n{schedule}");
    let code = match result {
        Ok(run) => {
            println!("outcome:   {:?}\ncounts:    {:?}", run.outcome, run.counts);
            println!("events:");
            for e in &run.events {
                println!(
                    "  {:>3} {:>14} {:?} {}",
                    e.seq,
                    e.at.as_millis(),
                    e.origin,
                    e.kind
                );
            }
            println!("result:    pass");
            ExitCode::SUCCESS
        }
        Err(why) => {
            println!("result:    FAIL: {why}");
            ExitCode::FAILURE
        }
    };
    println!("log:       {}", dir.display());
    code
}

fn run_sweep(from: &str, to: &str, threads: Option<&str>, root: PathBuf) -> ExitCode {
    let (Ok(from), Ok(to)) = (from.parse::<u64>(), to.parse::<u64>()) else {
        return usage();
    };
    let threads = match threads.map(str::parse::<usize>) {
        None => std::thread::available_parallelism().map_or(1, usize::from),
        Some(Ok(n)) if n > 0 => n,
        Some(_) => return usage(),
    };
    let started = std::time::Instant::now();
    let result = sweep(from..to, &root, threads);
    println!(
        "{} seeds ({from}..{to}) on {threads} threads in {:.1?}",
        result.runs,
        started.elapsed()
    );
    println!("profiles:  {:?}", result.profiles);
    println!("outcomes:  {:?}", result.outcomes);
    println!("held because the gate was down: {}", result.held_by_outage);
    println!("merges:    {:?}", result.merges);
    let gaps = result.gaps();
    for gap in &gaps {
        println!("GAP: {gap}");
    }
    for f in &result.failures {
        println!(
            "FAIL seed {} ({}): {}\n  reproduce: {}",
            f.seed,
            f.profile,
            f.why,
            reproduce(f.seed)
        );
    }
    // Every seed asked for ran, or the sweep did not cover its range and is not a pass.
    let short = result.runs != to.saturating_sub(from);
    if short {
        println!(
            "SHORT: {} of {} seeds ran",
            result.runs,
            to.saturating_sub(from)
        );
    }
    if result.failures.is_empty() && gaps.is_empty() && !short {
        let _ = std::fs::remove_dir_all(&root);
        println!("pass");
        ExitCode::SUCCESS
    } else {
        println!(
            "{} failing seeds, {} gaps; failing logs are under {}",
            result.failures.len(),
            gaps.len(),
            root.display()
        );
        ExitCode::FAILURE
    }
}
