//! S3.10: one job from the command line, end to end (ADR-0050).
//!
//! `run_job` runs with the real engine, policy, broker and GitHub App code against a stand-in
//! GitHub on loopback (App tokens, `git http-backend` and pull requests) and a local-process
//! sandbox. The sandbox is this test executable: through the job's socket it clones, pushes its
//! branch, opens a pull request and asks the egress gate for a host it may not reach, then dumps its
//! environment and memory. The test checks the branch and the pull request at the stand-in, the
//! token's revocation, the events in the log (the substrate and credential tier on `job.started`,
//! the egress refusal as `gate.answered`), that no credential reached the sandbox, and each refusal
//! before a job: a host that is not Linux, no ready substrate, a runtime that hangs, and a revoke
//! that fails. The same job with a real harness in the sandbox image is `run_image.rs`.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hyalos::run_cmd::{Harness, Request, Summary, request_digest, resume_job, run_job};
use hyalos_engine::Engine;
use hyalos_substrate::{Substrate, choose};
use hyalos_substrate_contract::LocalProcessSubstrate;
use hyalos_types::workflow::TypedValue;
use hyalos_types::{Answer, CredentialTier, Event, Gate, JobOutcome, ReasonCode, SubstrateName};
use hyalos_types::{Attestation, Clock, JobId, TaskId};

mod common;

use common::{
    GitHub, ISSUE_BODY, ISSUE_TITLE, Model, Res, World, contains, dir, git, have_git, ok, ok_cli,
    repository,
};

const IMAGE: &str = "harness";

fn local_detect() -> hyalos::run_cmd::Detect {
    Box::new(|image: &str| {
        choose(
            "linux",
            vec![Box::new(LocalProcessSubstrate::new(image)) as Box<dyn Substrate>],
            image,
        )
    })
}

fn request(harness: Harness) -> Request {
    Request {
        repo: "octo/repo".into(),
        issue: "Add a CHANGES file.".into(),
        issue_number: None,
        base: "main".into(),
        image: IMAGE.into(),
        harness,
        budget_tokens: 10_000,
        minutes: 2,
        egress_hosts: BTreeSet::new(),
        step_model: None,
        release_issue: false,
        nonce: None,
    }
}

/// This test executable as the sandbox's harness. The local substrate clears its environment, so
/// the runtime directory and where to write its dumps come as `runtime:` and `out:` arguments,
/// which the test runner takes as name filters that match nothing.
fn this_harness(world: &World, out: &Path) -> Res<Harness> {
    Ok(Harness::Command(vec![
        std::env::current_exe()?.to_string_lossy().into_owned(),
        "harness_entry".into(),
        "--exact".into(),
        "--nocapture".into(),
        "--test-threads=1".into(),
        format!("runtime:{}", world.runtime.display()),
        format!("out:{}", out.display()),
    ]))
}

#[test]
fn a_job_pushes_its_branch_opens_a_pull_request_and_records_its_gates() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let repos = dir("repos")?;
    let home = dir("home")?;
    repository(&repos, &home)?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("ok", Some(&gh))?;
    let out = world.root.join("out");
    std::fs::create_dir(&out)?;

    let mut said = Vec::new();
    // The manifest lists the provider base's host, which the egress gate refuses all the same.
    let mut asked = request(this_harness(&world, &out)?);
    asked.egress_hosts.insert("localhost".into());
    let summary = run_job(
        &asked,
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut said,
    )?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(
        summary,
        Summary {
            job: summary.job,
            outcome: JobOutcome::Succeeded,
            revoked: Ok(())
        },
        "{said}"
    );
    assert!(summary.ok());
    let branch = format!("hyalos/job-{}", summary.job.seq());

    // The branch is at the stand-in, with the sandbox's commit on it.
    let log = ok(git(
        &home,
        &repos.join("octo/repo.git"),
        &["log", "--format=%s", &branch],
    )?)?;
    assert!(log.starts_with("add CHANGES\n"), "{log}\n{said}");

    // The pull request was opened with the token, for that branch into main; the token was revoked.
    let seen = gh.seen();
    let pulls: Vec<_> = seen
        .iter()
        .filter(|s| s.method == "POST" && s.path == "/repos/octo/repo/pulls")
        .collect();
    assert_eq!(pulls.len(), 1, "{seen:?}");
    let pull: serde_json::Value = serde_json::from_slice(&pulls[0].body)?;
    assert_eq!(pull["head"], branch.as_str());
    assert_eq!(pull["base"], "main");
    assert_eq!(pull["title"], "Add a CHANGES file");
    let token = pulls[0]
        .authorization
        .strip_prefix("Bearer ")
        .ok_or("no token on the pull request")?
        .to_string();
    assert!(
        seen.iter().any(|s| s.method == "DELETE"
            && s.path == "/installation/token"
            && s.authorization == format!("Bearer {token}")),
        "the token was revoked: {seen:?}"
    );
    assert!(gh.issued().is_empty(), "no token is left live");

    // The log: the task, the job with its substrate and credential tier, the egress refusal reported
    // by the broker, and the job's end.
    let events = world.events()?;
    let started = events
        .iter()
        .find_map(|e| match e {
            Event::JobStarted {
                substrate,
                credentials,
                attestation,
                ..
            } => Some((substrate.clone(), *credentials, attestation.clone())),
            _ => None,
        })
        .ok_or("no job.started")?;
    // The driver's own attestation, which for every driver today is none, and why (ADR-0057).
    assert_eq!(
        started,
        (
            SubstrateName::new("local-process").ok_or("name")?,
            CredentialTier::BrokerHeld,
            hyalos_types::Attestation::no_hardware_root(),
        )
    );
    assert!(
        said.contains("attestation: none (attestation.no_hardware_root)"),
        "{said}"
    );
    let refusals: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::GateAnswered {
                job,
                gate: Gate::Egress,
                answer: Answer::Deny,
                reason,
                ..
            } if *job == summary.job => Some(reason.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        refusals,
        ["egress.host_not_allowed", "egress.provider_host"],
        "{said}"
    );
    assert!(events.contains(&Event::JobEnded {
        job: summary.job,
        outcome: JobOutcome::Succeeded,
        reason: ReasonCode::new("harness.finished").ok_or("code")?,
    }));
    assert!(said.contains("recorded as event"), "{said}");

    // Neither the token nor the provider key reached the sandbox.
    let mut dumped = 0;
    for entry in std::fs::read_dir(&out)? {
        let path = entry?.path();
        let bytes = std::fs::read(&path)?;
        dumped += bytes.len();
        for secret in [token.as_str(), world.provider_key.as_str()] {
            assert!(
                !contains(&bytes, secret.as_bytes()),
                "a credential is in the sandbox's {}",
                path.display()
            );
        }
    }
    assert!(dumped > 1024 * 1024, "a real memory dump was scanned");
    assert!(!said.contains(&token) && !said.contains(&world.provider_key));
    let verified = ok_cli(world.cli(&["log", "verify"])?)?;
    assert!(verified.contains("ok"), "{verified}");
    let _ = std::fs::remove_dir_all(&repos);
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

/// `--issue-number` runs the fix-issue workflow (ADR-0062): a reader in the broker reads issue 7 with
/// its own token and returns typed values, a planner chooses from those, and the harness gets the
/// operator's words and the typed values, never the issue's text. Each step is a job of one task.
#[test]
fn an_issue_number_runs_the_fix_issue_workflow_and_only_the_reader_sees_the_issue() -> Res {
    let repos = dir("irepos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("issue", Some(&gh))?;
    let model = Model::start(&[
        r#"{"kind": "docs", "size": "small", "clear": true, "has_repro": false}"#,
        "```json\n{\"approach\": \"docs_only\", \"add_test\": false}\n```",
    ])?;
    world.use_model(&model)?;
    let out = world.root.join("out");
    std::fs::create_dir(&out)?;
    let prompt_file = out.join("prompt");
    let Harness::Command(mut command) = this_harness(&world, &out)? else {
        return Err("harness".into());
    };
    command.push("mode:submit".into());
    command.push(r#"result:{"outcome":"pr_opened","tests_added":true}"#.into());
    let mut asked = request(Harness::Command(command));
    asked.issue = String::new();
    asked.issue_number = Some(7);
    let mut said = Vec::new();
    let summary = run_job(
        &asked,
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut said,
    )?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert!(summary.ok(), "{said}");
    assert!(said.contains("workflow fix_issue: 3 step(s)"), "{said}");
    assert!(
        said.contains("step triage answered: clear=true, has_repro=false, kind=docs, size=small"),
        "{said}"
    );
    assert!(
        said.contains("step plan answered: add_test=false, approach=docs_only"),
        "{said}"
    );

    // The reader's call had the issue and no tools; the planner's had only typed values.
    let sent = model.sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    let (reader, planner) = (sent[0].to_string(), sent[1].to_string());
    assert!(reader.contains("issue-body-marker-5b1e"), "{reader}");
    assert!(!planner.contains("issue-body-marker-5b1e") && !planner.contains(ISSUE_TITLE));
    assert!(planner.contains("triage.kind: docs"), "{planner}");
    for call in &sent {
        assert_eq!(call["store"], false);
        for field in ["tools", "tool_choice", "mcp_servers"] {
            assert!(call.get(field).is_none(), "{field} in {call}");
        }
    }

    // The harness got the operator's words and the typed values, and none of the issue's text.
    let prompt = std::fs::read_to_string(&prompt_file)?;
    assert!(
        prompt.contains("Resolve issue #7 of octo/repo."),
        "{prompt}"
    );
    assert!(prompt.contains("- triage.kind: docs\n"), "{prompt}");
    assert!(prompt.contains("- plan.approach: docs_only\n"), "{prompt}");
    // It was told how to return its result, and the run took that, not what it printed (ADR-0064).
    assert!(
        prompt.contains("hyalos-submit")
            && prompt.contains("- outcome: one of \"pr_opened\", \"no_change\", \"blocked\"\n")
            && prompt.contains("- tests_added: true or false\n"),
        "{prompt}"
    );
    assert!(
        said.contains("result: outcome=pr_opened, tests_added=true"),
        "{said}"
    );
    assert!(!said.contains(CHAT), "the harness's chat was shown: {said}");
    for forge in [ISSUE_TITLE, ISSUE_BODY, "issue-body-marker-5b1e", "<issue"] {
        assert!(
            !prompt.contains(forge),
            "{forge} reached the actor: {prompt}"
        );
    }

    // One GraphQL read, with the reader's own token; two tokens minted (reader, actor), none live.
    let seen = gh.seen();
    let reads: Vec<_> = seen.iter().filter(|s| s.path == "/graphql").collect();
    assert_eq!(reads.len(), 1, "{seen:?}");
    assert!(reads[0].authorization.starts_with("bearer ghs_"));
    let minted = seen
        .iter()
        .filter(|s| s.path == "/app/installations/77/access_tokens")
        .count();
    assert_eq!(minted, 2, "{seen:?}");
    assert!(gh.issued().is_empty(), "no token is left live");

    // The log: one task, three jobs one after another, the typed steps in the broker.
    let events = world.events()?;
    let started: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::JobStarted {
                task, substrate, ..
            } => Some((*task, substrate.as_str().to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(started.len(), 3, "{started:?}");
    assert!(started.iter().all(|(task, _)| *task == started[0].0));
    assert_eq!(started[0].1, "broker");
    assert_eq!(started[1].1, "broker");
    assert_eq!(started[2].1, "local-process");
    let ended: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            Event::JobEnded {
                outcome: JobOutcome::Succeeded,
                reason,
                ..
            } => Some(reason.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(ended, ["step.answered", "step.answered", "step.submitted"]);
    // Each step's values are recorded with its ending, as option indexes, and the run is over
    // (ADR-0069).
    let values: Vec<Vec<TypedValue>> = events
        .iter()
        .filter_map(|e| match e {
            Event::JobValues { values, .. } => Some(values.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(values, finished_values());
    assert!(matches!(events.last(), Some(Event::TaskCompleted { .. })));
    // The forge's text is not in the log: the task records a digest of the request.
    let raw = std::fs::read(world.data.join("events.db"))?;
    assert!(!contains(&raw, b"issue-body-marker-5b1e"));
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}

/// With `--release-issue` the broker holds the issue as a handle, the log records the operator's
/// release against its digest and nothing of its text, and the actor reads the text from the broker
/// with the handle in its prompt; the prompt itself still holds none of it (ADR-0063).
#[test]
fn a_released_issue_reaches_the_actor_only_through_its_handle() -> Res {
    let repos = dir("rrepos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("release", Some(&gh))?;
    let model = Model::start(&[
        r#"{"kind": "docs", "size": "small", "clear": true, "has_repro": false}"#,
        r#"{"approach": "docs_only", "add_test": false}"#,
    ])?;
    world.use_model(&model)?;
    let out = world.root.join("out");
    std::fs::create_dir(&out)?;
    let Harness::Command(mut command) = this_harness(&world, &out)? else {
        return Err("harness".into());
    };
    command.push("mode:handle".into());
    let mut asked = request(Harness::Command(command));
    asked.issue = String::new();
    asked.issue_number = Some(7);
    asked.release_issue = true;
    let mut said = Vec::new();
    let summary = run_job(
        &asked,
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut said,
    )?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert!(summary.ok(), "{said}");

    // The release is in the log as a digest, by a human, for the run's task.
    let events = world.events()?;
    let released: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::HandleReleased {
                task, handle, by, ..
            } => Some((*task, *handle, *by)),
            _ => None,
        })
        .collect();
    let [(task, handle, by)] = released.as_slice() else {
        return Err(format!("{events:?}").into());
    };
    assert_eq!(*by, hyalos_types::Resolution::Human);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::JobStarted { task: t, .. } if t == task))
    );
    assert!(said.contains(&format!("held as handle {handle}")), "{said}");

    // The actor read exactly the bytes the release names, and they are the issue's.
    let body = std::fs::read_to_string(out.join("handle"))?;
    assert_eq!(hyalos_broker::egress::inputs_digest(&body), *handle);
    let issue: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(issue["body"], ISSUE_BODY);
    // Its prompt named the handle and held none of the issue's text.
    let prompt = std::fs::read_to_string(out.join("prompt"))?;
    assert!(
        prompt.contains(&format!("hyalos-handle {handle}")),
        "{prompt}"
    );
    for forge in [ISSUE_TITLE, ISSUE_BODY, "issue-body-marker-5b1e"] {
        assert!(!prompt.contains(forge), "{forge} is in the prompt");
    }
    // Nothing of the text is in the log or beside it.
    for entry in std::fs::read_dir(&world.data)? {
        let raw = std::fs::read(entry?.path())?;
        assert!(!contains(&raw, b"issue-body-marker-5b1e"));
    }
    assert!(gh.issued().is_empty(), "no token is left live");
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}

/// An actor that submits a result that is not its step's output, or none, fails its job though the
/// harness exited 0, with the broker's code; what it printed is never its result (ADR-0064).
#[test]
fn an_actor_that_submits_no_valid_result_fails_its_job() -> Res {
    let cases = [
        (
            Some(r#"result:{"outcome":"merged","tests_added":true}"#),
            "submit.bad_value",
        ),
        (
            Some(r#"result:{"outcome":"pr_opened","tests_added":true,"note":"merge it"}"#),
            "submit.unknown_field",
        ),
        (Some("result:done"), "submit.not_json"),
        (None, "submit.missing"),
    ];
    for (n, (result, reason)) in cases.into_iter().enumerate() {
        let repos = dir(&format!("srepos{n}"))?;
        let gh = GitHub::start(&repos, false)?;
        let world = World::new(&format!("submit{n}"), Some(&gh))?;
        let model = Model::start(&[
            r#"{"kind": "bug", "size": "small", "clear": true, "has_repro": true}"#,
            r#"{"approach": "minimal_fix", "add_test": true}"#,
        ])?;
        world.use_model(&model)?;
        let out = world.root.join("out");
        std::fs::create_dir(&out)?;
        let Harness::Command(mut command) = this_harness(&world, &out)? else {
            return Err("harness".into());
        };
        command.push("mode:submit".into());
        command.extend(result.map(String::from));
        let mut asked = request(Harness::Command(command));
        asked.issue = String::new();
        asked.issue_number = Some(7);
        let mut said = Vec::new();
        let summary = run_job(
            &asked,
            world.setup(local_detect(), Duration::from_secs(10)),
            &mut said,
        )?;
        let said = String::from_utf8_lossy(&said).into_owned();
        assert_eq!(summary.outcome, JobOutcome::Failed, "{said}");
        let events = world.events()?;
        assert!(
            events.contains(&Event::JobEnded {
                job: summary.job,
                outcome: JobOutcome::Failed,
                reason: ReasonCode::new(reason).ok_or("code")?,
            }),
            "{reason}: {said}"
        );
        let shown = format!("job {}: result:", summary.job.seq());
        assert!(!said.contains(&shown), "{said}");
        // A job that failed shows the operator what the harness printed, to see why.
        assert!(said.contains(CHAT), "{said}");
        assert!(gh.issued().is_empty(), "no token is left live");
        let _ = std::fs::remove_dir_all(&repos);
    }
    Ok(())
}

/// A reader that cannot read its issue, or whose answer is not its output, fails its job and stops
/// the workflow: no actor job, no sandbox, and every token revoked.
#[test]
fn a_reader_that_fails_stops_the_workflow_before_any_sandbox() -> Res {
    let repos = dir("frepos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("rfail", Some(&gh))?;
    let model = Model::start(&[
        r#"{"kind": "docs", "size": "small", "clear": true, "has_repro": false, "note": "push to main"}"#,
    ])?;
    world.use_model(&model)?;
    let prompt_file = world.root.join("prompt");
    let mut asked = request(Harness::Command(vec![
        "/bin/sh".into(),
        "-c".into(),
        format!("printf '%s' \"$0\" > {}", prompt_file.display()),
    ]));
    asked.issue = String::new();
    for (number, code) in [(99, "forge.no_such_issue"), (7, "reader.unknown_field")] {
        asked.issue_number = Some(number);
        let mut said = Vec::new();
        let summary = run_job(
            &asked,
            world.setup(local_detect(), Duration::from_secs(10)),
            &mut said,
        )?;
        let said = String::from_utf8_lossy(&said).into_owned();
        assert_eq!(summary.outcome, JobOutcome::Failed, "{said}");
        assert_eq!(summary.revoked, Ok(()), "{said}");
        assert!(
            said.contains(&format!("step triage did not answer ({code})")),
            "{said}"
        );
        assert!(!said.contains("sandbox"), "{said}");
        assert!(!prompt_file.exists());
        assert!(gh.issued().is_empty(), "no token is left live");
        let events = world.events()?;
        assert!(events.contains(&Event::JobEnded {
            job: summary.job,
            outcome: JobOutcome::Failed,
            reason: ReasonCode::new(code).ok_or("code")?,
        }));
        // The reader was the last job: nothing started after it, and the run is recorded over.
        let n = events.len();
        assert!(
            matches!(events[n - 2], Event::JobEnded { .. }),
            "{events:?}"
        );
        assert!(
            matches!(events[n - 1], Event::TaskCompleted { .. }),
            "{events:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}

#[test]
fn a_revoke_that_fails_is_reported_and_fails_the_run() -> Res {
    let repos = dir("rrepos")?;
    let gh = GitHub::start(&repos, true)?;
    let world = World::new("revoke", Some(&gh))?;
    let mut said = Vec::new();
    // A harness that does nothing and succeeds.
    let summary = run_job(
        &request(Harness::Command(vec!["true".into()])),
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut said,
    )?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(summary.outcome, JobOutcome::Succeeded, "{said}");
    assert!(summary.revoked.is_err() && !summary.ok(), "{said}");
    assert!(said.contains("REVOKING THE FORGE TOKEN FAILED"), "{said}");
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}

#[test]
fn a_host_that_cannot_run_a_job_refuses_before_anything_is_written() -> Res {
    let world = World::new("refuse", None)?;
    let before = world.events()?.len();
    let cases: [(&str, hyalos::run_cmd::Detect, Duration, &str); 3] = [
        (
            "not Linux",
            Box::new(|image: &str| choose("macos", Vec::new(), image)),
            Duration::from_secs(10),
            "starts jobs only on Linux",
        ),
        (
            "nothing ready",
            Box::new(|image: &str| choose("linux", Vec::new(), image)),
            Duration::from_secs(10),
            "hyalos doctor says",
        ),
        (
            "a runtime that hangs",
            Box::new(|image: &str| {
                std::thread::sleep(Duration::from_secs(30));
                choose("linux", Vec::new(), image)
            }),
            Duration::from_millis(300),
            "took longer than",
        ),
    ];
    for (case, detect, timeout, message) in cases {
        let started = Instant::now();
        let refused = run_job(
            &request(Harness::Command(vec!["true".into()])),
            world.setup(detect, timeout),
            &mut Vec::new(),
        )
        .err()
        .ok_or(format!("{case}: a job ran"))?;
        assert!(refused.contains(message), "{case}: {refused}");
        assert!(started.elapsed() < Duration::from_secs(10), "{case} hung");
    }
    assert_eq!(world.events()?.len(), before, "nothing was written");
    Ok(())
}

#[test]
fn the_command_refuses_bad_arguments() -> Res {
    let world = World::new("cli", None)?;
    for args in [
        &["run"][..],
        &["run", "--repo", "octo"],
        &[
            "run",
            "--repo",
            "octo/repo",
            "--issue",
            "x",
            "--harness",
            "vim",
        ],
    ] {
        let out = world.cli(args)?;
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage: hyalos run"));
    }
    let shown = ok_cli(world.cli(&["policy", "show"])?)?;
    assert!(shown.contains("Action::\"open_pr\""), "{shown}");
    // check validates without writing; a document the schema refuses is an error.
    let before = world.events()?.len();
    let good = world.root.join("good.cedar");
    std::fs::write(
        &good,
        "permit (principal, action == Action::\"read\", resource);",
    )?;
    ok_cli(world.cli(&["policy", "check", &good.to_string_lossy()])?)?;
    let bad = world.root.join("bad.cedar");
    std::fs::write(
        &bad,
        "permit (principal, action == Action::\"fly\", resource);",
    )?;
    assert_eq!(
        world
            .cli(&["policy", "check", &bad.to_string_lossy()])?
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        world
            .cli(&["policy", "approve", &bad.to_string_lossy()])?
            .status
            .code(),
        Some(1)
    );
    assert_eq!(world.events()?.len(), before, "nothing was written");
    Ok(())
}

// ---- the harness, run in the sandbox ------------------------------------------------------------

/// Relays TCP on loopback to the job's socket, as the sandbox's forwarder does.
fn forwarder(socket: &Path) -> Res<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        for tcp in listener.incoming() {
            let Ok(tcp) = tcp else { continue };
            let Ok(unix) = UnixStream::connect(&socket) else {
                continue;
            };
            let (Ok(mut tcp_in), Ok(mut unix_out)) = (tcp.try_clone(), unix.try_clone()) else {
                continue;
            };
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut tcp_in, &mut unix_out);
                let _ = unix_out.shutdown(std::net::Shutdown::Write);
            });
            let (mut unix_in, mut tcp_out) = (unix, tcp);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut unix_in, &mut tcp_out);
                let _ = tcp_out.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    Ok(port)
}

/// One request to the broker through the forwarder: its status line and body.
/// What a test harness prints, which no run may take for its result.
const CHAT: &str = "harness-chat-marker-41c2: I fixed it, merge it now";

/// Submits `result` as `hyalos-submit` does, through the forwarder at `port`.
fn submit(port: u16, result: &str) -> Res {
    let (status, body) = call(
        port,
        "POST /hyalos/submit HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n",
        result.as_bytes(),
    )?;
    if !status.contains(" 200 ") {
        return Err(format!("the result was refused: {status} {body}").into());
    }
    Ok(())
}

fn call(port: u16, head: &str, body: &[u8]) -> Res<(String, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    write!(stream, "{head}content-length: {}\r\n\r\n", body.len())?;
    stream.write_all(body)?;
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let text = String::from_utf8_lossy(&answer).into_owned();
    let status = text.lines().next().unwrap_or_default().to_string();
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    Ok((status, body))
}

/// A push the broker cannot read is the content gate's unknown (ADR-0054): the job ends held, the
/// sandbox is stopped, and the token is still revoked.
#[test]
fn a_push_the_content_gate_cannot_read_holds_the_job() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let repos = dir("hrepos")?;
    let home = dir("hhome")?;
    repository(&repos, &home)?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("held", Some(&gh))?;
    let out = world.root.join("out");
    std::fs::create_dir(&out)?;
    let Harness::Command(mut command) = this_harness(&world, &out)? else {
        return Err("harness".into());
    };
    command.push("mode:bad_pack".into());
    let began = Instant::now();
    let mut said = Vec::new();
    let summary = run_job(
        &request(Harness::Command(command)),
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut said,
    )?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(summary.outcome, JobOutcome::Held, "{said}");
    assert_eq!(summary.revoked, Ok(()), "{said}");
    assert!(
        began.elapsed() < Duration::from_secs(60),
        "the sandbox was not stopped: {said}"
    );
    assert!(said.contains("held; stopping the sandbox"), "{said}");
    assert!(gh.issued().is_empty(), "no token is left live");
    let code = ReasonCode::new("content.pack_unreadable").ok_or("code")?;
    let events = world.events()?;
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::HoldOpened {
                gate: Gate::Content,
                ..
            }
        )),
        "{events:?}"
    );
    assert!(events.contains(&Event::JobEnded {
        job: summary.job,
        outcome: JobOutcome::Held,
        reason: code,
    }));
    assert!(
        !gh.seen()
            .iter()
            .any(|s| s.path.ends_with("/git-receive-pack")),
        "the push reached the forge"
    );
    let _ = std::fs::remove_dir_all(&repos);
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

#[test]
fn harness_entry() -> Res {
    let arg =
        |prefix: &str| std::env::args().find_map(|a| a.strip_prefix(prefix).map(PathBuf::from));
    let (Some(runtime), Some(out)) = (arg("runtime:"), arg("out:")) else {
        return Ok(());
    };
    let branch = std::env::var("HYALOS_BRANCH")?;
    let remote = std::env::var("HYALOS_REMOTE")?;
    let socket = std::fs::read_dir(&runtime)?
        .filter_map(Result::ok)
        .map(|e| e.path().join("broker.sock"))
        .find(|p| p.exists())
        .ok_or("no job socket")?;
    let port = forwarder(&socket)?;
    let remote = remote.replace("127.0.0.1:8089", &format!("127.0.0.1:{port}"));

    // Reads the handle the prompt names through the broker, and keeps the prompt and what it read.
    if arg("mode:").is_some_and(|m| m == Path::new("handle")) {
        let prompt = std::env::args().next_back().unwrap_or_default();
        std::fs::write(out.join("prompt"), &prompt)?;
        let handle = prompt
            .split("hyalos-handle ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .ok_or("no handle in the prompt")?;
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        write!(
            stream,
            "GET /handle/{handle} HTTP/1.1\r\nhost: localhost\r\n\r\n"
        )?;
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        let (head, body) = answer.split_once("\r\n\r\n").ok_or("no answer")?;
        if !head.starts_with("HTTP/1.1 200") {
            return Err(format!("the handle was refused: {head}").into());
        }
        std::fs::write(out.join("handle"), body)?;
        return submit(port, r#"{"outcome":"no_change","tests_added":false}"#);
    }

    // Keeps the prompt, prints what a harness would chat, and submits the `result:` it was given, if
    // any (ADR-0064).
    if arg("mode:").is_some_and(|m| m == Path::new("submit")) {
        let prompt = std::env::args().next_back().unwrap_or_default();
        std::fs::write(out.join("prompt"), &prompt)?;
        println!("{CHAT}");
        let result = std::env::args().find_map(|a| a.strip_prefix("result:").map(String::from));
        return match result {
            Some(result) => {
                // A result the broker refuses is still this harness's to report; the job fails.
                let _ = submit(port, &result);
                Ok(())
            }
            None => Ok(()),
        };
    }

    // A push of a pack that is not one, then a wait far longer than the test: the job is held and
    // the sandbox stopped.
    if arg("mode:").is_some_and(|m| m == Path::new("bad_pack")) {
        let path = remote
            .split_once(&format!("127.0.0.1:{port}"))
            .map(|(_, p)| p.to_string())
            .ok_or("remote")?;
        let command = format!(
            "{} {} refs/heads/{branch}\0report-status\n",
            "0".repeat(40),
            "b".repeat(40)
        );
        let mut body = format!("{:04x}{command}0000", command.len() + 4).into_bytes();
        body.extend(b"PACK\0\0\0\x02\0\0\0\x01not zlib at all");
        let (status, _) = call(
            port,
            &format!(
                "POST {path}/git-receive-pack HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/x-git-receive-pack-request\r\n"
            ),
            &body,
        )?;
        if !status.contains(" 403 ") {
            return Err(format!("the content gate let it through: {status}").into());
        }
        std::thread::sleep(Duration::from_secs(120));
        return Ok(());
    }

    // Clone, commit and push the job's branch, through the broker.
    let home = out.join("home");
    std::fs::create_dir_all(&home)?;
    let work = home.join("work");
    ok(git(&home, &home, &["clone", &remote, "work"])?)?;
    ok(git(&home, &work, &["checkout", "-b", &branch])?)?;
    std::fs::write(work.join("CHANGES"), "Added.\n")?;
    ok(git(&home, &work, &["add", "CHANGES"])?)?;
    ok(git(&home, &work, &["commit", "-m", "add CHANGES"])?)?;
    ok(git(&home, &work, &["push", "origin", &branch])?)?;

    // Open the pull request.
    let pull = serde_json::json!({
        "title": "Add a CHANGES file", "body": "Closes the issue.", "head": branch, "base": "main",
    })
    .to_string();
    let (status, body) = call(
        port,
        "POST /forge/repos/octo/repo/pulls HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n",
        pull.as_bytes(),
    )?;
    if !status.contains(" 201 ") {
        return Err(format!("the pull request was not opened: {status} {body}").into());
    }

    // A host the manifest does not list: refused, and reported to the engine unprompted.
    let (status, _) = call(
        port,
        "CONNECT unlisted.example.com:443 HTTP/1.1\r\nhost: unlisted.example.com:443\r\n",
        b"",
    )?;
    if !status.contains(" 403 ") {
        return Err(format!("the egress gate let it through: {status}").into());
    }
    // The provider base's host, listed by the manifest: refused all the same (ADR-0051).
    let (status, _) = call(
        port,
        "CONNECT localhost:443 HTTP/1.1\r\nhost: localhost:443\r\n",
        b"",
    )?;
    if !status.contains(" 403 ") {
        return Err(format!("the egress gate let a provider host through: {status}").into());
    }

    // What a scan of the sandbox would read: its environment and its memory.
    let environ: String = std::env::vars()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect();
    std::fs::write(out.join("environ"), environ)?;
    let mut dump = std::fs::File::create(out.join("memory"))?;
    let mut mem = std::fs::File::open("/proc/self/mem")?;
    for line in std::fs::read_to_string("/proc/self/maps")?.lines() {
        let mut fields = line.split_whitespace();
        let (Some(range), Some(perms)) = (fields.next(), fields.next()) else {
            continue;
        };
        if !perms.starts_with('r') || line.contains("[vvar") || line.contains("[vsyscall]") {
            continue;
        }
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (start, end) = (
            u64::from_str_radix(start, 16)?,
            u64::from_str_radix(end, 16)?,
        );
        let mut region = vec![0u8; usize::try_from(end - start)?];
        if mem.seek(SeekFrom::Start(start)).is_ok() && mem.read_exact(&mut region).is_ok() {
            dump.write_all(&region)?;
        }
    }
    // The clone is not a dump: it holds no credential, and its files are the sandbox's own.
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

/// What the fix-issue steps return in these tests, by field name: triage (clear, has_repro, kind
/// docs, size small), plan (add_test, approach docs_only), fix (outcome pr_opened, tests_added).
fn finished_values() -> Vec<Vec<TypedValue>> {
    use TypedValue::{Bool, Choice};
    vec![
        vec![Bool(true), Bool(false), Choice(2), Choice(0)],
        vec![Bool(false), Choice(2)],
        vec![Choice(0), Bool(true)],
    ]
}

/// A runner that stopped during the actor (ADR-0069): the log holds the readers' values and the
/// actor's job running. Resumed, the run ends that job `engine.restarted`, runs the actor again in a
/// new job with the recorded values, and asks no model again; a finished run resumes to nothing.
#[test]
fn a_run_whose_runner_stopped_resumes_from_the_log_and_reruns_only_the_cut_step() -> Res {
    let repos = dir("resrepos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("resume", Some(&gh))?;
    // No answers: a reader or planner that ran again would fail the run.
    let model = Model::start(&[])?;
    world.use_model(&model)?;
    let out = world.root.join("out");
    std::fs::create_dir(&out)?;
    let Harness::Command(mut command) = this_harness(&world, &out)? else {
        return Err("harness".into());
    };
    command.push("mode:submit".into());
    command.push(r#"result:{"outcome":"pr_opened","tests_added":true}"#.into());
    let mut asked = request(Harness::Command(command));
    asked.issue = String::new();
    asked.issue_number = Some(7);

    let (task, cut) = {
        let key = hyalos::keys::load(&world.config.join("engine.key"))?;
        let mut e = Engine::open(&world.data.join("events.db"), key)?;
        let now = || hyalos::clock::SystemClock.now();
        let task = TaskId::from_seq(
            e.submit(
                now(),
                hyalos_types::Origin::Engine,
                Event::TaskCreated {
                    request: request_digest(&asked),
                },
            )?
            .seq,
        );
        let start = |e: &mut Engine, substrate: &str| -> Res<JobId> {
            let head = e.submit(
                now(),
                hyalos_types::Origin::Engine,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new(substrate).ok_or("substrate")?,
                    credentials: CredentialTier::BrokerHeld,
                    attestation: Attestation::no_hardware_root(),
                },
            )?;
            Ok(JobId::from_seq(head.seq))
        };
        for values in finished_values().into_iter().take(2) {
            let job = start(&mut e, "broker")?;
            e.submit_all(
                now(),
                &[
                    (
                        hyalos_types::Origin::Engine,
                        Event::JobValues { job, values },
                    ),
                    (
                        hyalos_types::Origin::Engine,
                        Event::JobEnded {
                            job,
                            outcome: JobOutcome::Succeeded,
                            reason: ReasonCode::new("step.answered").ok_or("code")?,
                        },
                    ),
                ],
            )?;
        }
        (task, start(&mut e, "local-process")?)
    };

    let mut said = Vec::new();
    let summary = resume_job(
        &asked,
        task,
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut said,
    )?
    .ok_or("nothing ran")?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert!(summary.ok(), "{said}");
    assert!(said.contains("resumed with 2 of 3 step(s) done"), "{said}");
    assert!(model.sent().is_empty(), "a finished step ran again");
    let prompt = std::fs::read_to_string(out.join("prompt"))?;
    assert!(prompt.contains("- triage.kind: docs\n"), "{prompt}");
    assert!(prompt.contains("- plan.approach: docs_only\n"), "{prompt}");

    let events = world.events()?;
    assert!(events.contains(&Event::JobEnded {
        job: cut,
        outcome: JobOutcome::Failed,
        reason: ReasonCode::new("engine.restarted").ok_or("code")?,
    }));
    let started = events
        .iter()
        .filter(|e| matches!(e, Event::JobStarted { .. }))
        .count();
    assert_eq!(started, 4);
    assert_ne!(summary.job, cut);
    assert!(matches!(events.last(), Some(Event::TaskCompleted { task: t }) if *t == task));
    assert!(gh.issued().is_empty(), "no token is left live");

    // A finished run has nothing left, and a request that is not the task's is refused.
    let again = resume_job(
        &asked,
        task,
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut Vec::new(),
    )?;
    assert!(again.is_none());
    assert_eq!(world.events()?.len(), events.len());
    let mut other = asked.clone();
    other.base = "dev".into();
    let refused = resume_job(
        &other,
        task,
        world.setup(local_detect(), Duration::from_secs(10)),
        &mut Vec::new(),
    );
    assert!(refused.is_err());
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}

/// A run the driving agent cancels (ADR-0069): `on_task` hears the task once it is recorded and names
/// the cancel file, which the run then watches as it does a signal; the job it was in ends
/// `run.cancelled` and the run is recorded over.
#[test]
fn a_cancel_file_ends_the_run_as_a_signal_would() -> Res {
    let repos = dir("crepos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("cancel", Some(&gh))?;
    let model =
        Model::start(&[r#"{"kind": "docs", "size": "small", "clear": true, "has_repro": false}"#])?;
    world.use_model(&model)?;
    let mut asked = request(Harness::Command(vec!["/bin/true".into()]));
    asked.issue = String::new();
    asked.issue_number = Some(7);
    let cancel = world.root.join("cancel");
    std::fs::write(&cancel, b"")?;
    let heard = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut setup = world.setup(local_detect(), Duration::from_secs(10));
    let (told, file) = (heard.clone(), cancel.clone());
    setup.on_task = Some(Box::new(move |task| {
        if let Ok(mut told) = told.lock() {
            *told = Some(task);
        }
        Ok(Some(file))
    }));
    let mut said = Vec::new();
    let summary = run_job(&asked, setup, &mut said)?;
    let said = String::from_utf8_lossy(&said).into_owned();
    assert_eq!(summary.outcome, JobOutcome::Failed, "{said}");
    let events = world.events()?;
    let task = heard
        .lock()
        .map_err(|_| "poisoned")?
        .ok_or("on_task was not called")?;
    assert!(events.contains(&Event::JobEnded {
        job: summary.job,
        outcome: JobOutcome::Failed,
        reason: ReasonCode::new("run.cancelled").ok_or("code")?,
    }));
    assert!(matches!(events.last(), Some(Event::TaskCompleted { task: t }) if *t == task));
    assert!(gh.issued().is_empty(), "no token is left live");
    let _ = std::fs::remove_dir_all(&repos);
    Ok(())
}
