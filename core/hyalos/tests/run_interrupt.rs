//! S3.10c: an interrupted run still ends its job and revokes its token (ADR-0053).
//!
//! `run_job` runs as in `run_job.rs`, with a sandbox that waits. Once it is running the test sends
//! this process `SIGINT`, then in a second run `SIGTERM` twice with a revoke that fails. Each time the
//! test checks that the sandbox was stopped, the token was revoked at the stand-in GitHub (or the
//! failure reported as at any end), `job.ended` is `failed` with `run.interrupted`, and the second
//! signal reached the run's `again`. In the binary, `again` exits at once.
//!
//! This file is its own test executable, so the signals it sends reach no other test.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hyalos::interrupt::Interrupt;
use hyalos::run_cmd::{Harness, Request, Summary, run_job};
use hyalos_substrate::{Substrate, choose};
use hyalos_substrate_contract::LocalProcessSubstrate;
use hyalos_types::{Event, JobOutcome, ReasonCode};
use rustix::process::{Signal, getpgid, getpid, kill_process};

mod common;

use common::{GitHub, Res, World, dir};

fn local_detect() -> hyalos::run_cmd::Detect {
    Box::new(|image: &str| {
        choose(
            "linux",
            vec![Box::new(LocalProcessSubstrate::new(image)) as Box<dyn Substrate>],
            image,
        )
    })
}

/// A job whose harness says it has started, then waits far longer than the test.
fn waiting(started: &Path) -> Request {
    Request {
        repo: "octo/repo".into(),
        issue: "Add a CHANGES file.".into(),
        issue_number: None,
        base: "main".into(),
        image: "harness".into(),
        harness: Harness::Command(vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("touch {}; exec sleep 120", started.display()),
        ]),
        budget_tokens: 10_000,
        minutes: 2,
        egress_hosts: BTreeSet::new(),
        step_model: None,
        release_issue: false,
        await_release: false,
        nonce: None,
        skills: Default::default(),
        workflow: None,
    }
}

fn wait_for(path: &Path) -> Res {
    let began = Instant::now();
    while !path.exists() {
        if began.elapsed() > Duration::from_secs(30) {
            return Err(format!("{} never appeared", path.display()).into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

type Ran = (Result<Summary, String>, String);

/// Starts the job on a thread, with signals taken by an [`Interrupt`] whose `again` reports here.
fn start(
    world: &World,
    request: Request,
) -> Res<(std::thread::JoinHandle<Ran>, mpsc::Receiver<i32>)> {
    let (again_tx, again) = mpsc::channel();
    let interrupt = Interrupt::on_signals(move |signal| {
        let _ = again_tx.send(signal);
    })?;
    let setup = world.setup_with(local_detect(), Duration::from_secs(10), interrupt);
    let run = std::thread::spawn(move || {
        let mut said = Vec::new();
        let summary = run_job(&request, setup, &mut said);
        (summary, String::from_utf8_lossy(&said).into_owned())
    });
    Ok((run, again))
}

fn interrupted() -> Res<ReasonCode> {
    Ok(ReasonCode::new("run.interrupted").ok_or("code")?)
}

#[test]
fn an_interrupted_run_stops_the_sandbox_revokes_the_token_and_records_the_end() -> Res {
    let repos = dir("sigrepos")?;

    // Ctrl-C while the sandbox runs.
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("sigint", Some(&gh))?;
    let started = world.root.join("started");
    let (run, again) = start(&world, waiting(&started))?;
    wait_for(&started)?;
    let began = Instant::now();
    kill_process(getpid(), Signal::INT)?;
    let (summary, said) = run.join().map_err(|_| "the run panicked")?;
    let summary = summary?;
    assert!(
        began.elapsed() < Duration::from_secs(20),
        "the sandbox was not stopped: {said}"
    );
    assert_eq!(summary.outcome, JobOutcome::Failed, "{said}");
    assert_eq!(summary.revoked, Ok(()), "{said}");
    assert!(!summary.ok());
    assert!(said.contains("interrupted (SIGINT)"), "{said}");
    assert!(said.contains("sandbox ended (Stopped)"), "{said}");
    assert!(said.contains("forge token revoked"), "{said}");
    // Revoked at the stand-in: a DELETE with the job's token, and none left live.
    assert!(
        gh.seen()
            .iter()
            .any(|s| s.method == "DELETE" && s.path == "/installation/token"),
        "{:?}",
        gh.seen()
    );
    assert!(gh.issued().is_empty(), "no token is left live");
    assert!(world.events()?.contains(&Event::JobEnded {
        job: summary.job,
        outcome: JobOutcome::Failed,
        reason: interrupted()?,
    }));
    assert!(again.try_recv().is_err(), "one signal is not two");

    // SIGTERM, twice, with a revoke that fails: the second reaches `again`, and the failed revoke is
    // reported as at any end.
    let gh = GitHub::start(&repos, true)?;
    let world = World::new("sigterm", Some(&gh))?;
    let started = world.root.join("started");
    let (run, again) = start(&world, waiting(&started))?;
    wait_for(&started)?;
    kill_process(getpid(), Signal::TERM)?;
    // Apart, so the two are taken as two; the run takes longer than this to end the job.
    std::thread::sleep(Duration::from_millis(50));
    kill_process(getpid(), Signal::TERM)?;
    let (summary, said) = run.join().map_err(|_| "the run panicked")?;
    let summary = summary?;
    assert_eq!(
        again.recv_timeout(Duration::from_secs(5))?,
        rustix::process::Signal::TERM.as_raw(),
        "{said}"
    );
    assert_eq!(summary.outcome, JobOutcome::Failed, "{said}");
    assert!(summary.revoked.is_err(), "{said}");
    assert!(said.contains("interrupted (SIGTERM)"), "{said}");
    assert!(said.contains("REVOKING THE FORGE TOKEN FAILED"), "{said}");
    assert!(world.events()?.contains(&Event::JobEnded {
        job: summary.job,
        outcome: JobOutcome::Failed,
        reason: interrupted()?,
    }));
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}

/// The broker is in a process group of its own, so the Ctrl-C a terminal sends the engine's group
/// does not reach it (ADR-0053).
#[test]
fn the_broker_is_in_its_own_process_group() -> Res {
    let runtime = dir("pgrp")?.join("r");
    hyalos::paths::make_private_dir(&runtime)?;
    let key = hyalos::launch::fresh_manifest_key()?;
    let broker = hyalos::launch::BrokerProcess::launch(
        Path::new(common::EXE),
        &runtime,
        &key.public(),
        Duration::from_secs(20),
    )?;
    let pid = rustix::process::Pid::from_raw(i32::try_from(broker.pid())?).ok_or("pid")?;
    let group = getpgid(Some(pid))?;
    assert_eq!(group, pid, "the broker leads its own group");
    assert_ne!(group, getpgid(None)?, "and it is not the engine's");
    Ok(())
}
