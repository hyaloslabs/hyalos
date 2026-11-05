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

use hyalos::run_cmd::{Harness, Request, Summary, run_job};
use hyalos_substrate::{Substrate, choose};
use hyalos_substrate_contract::LocalProcessSubstrate;
use hyalos_types::{Answer, CredentialTier, Event, Gate, JobOutcome, SubstrateName};

mod common;

use common::{
    GitHub, ISSUE_BODY, ISSUE_TITLE, Res, World, contains, dir, git, have_git, ok, ok_cli,
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
                ..
            } => Some((substrate.clone(), *credentials)),
            _ => None,
        })
        .ok_or("no job.started")?;
    assert_eq!(
        started,
        (
            SubstrateName::new("local-process").ok_or("name")?,
            CredentialTier::BrokerHeld
        )
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
        outcome: JobOutcome::Succeeded
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

/// `--issue-number`: the broker reads the issue with the job's token, and its text reaches the
/// harness set apart as the forge's users' (ADR-0052). A number with no issue fails the job before any
/// sandbox starts, and the token is still revoked.
#[test]
fn an_issue_read_from_github_reaches_the_harness_as_forge_text() -> Res {
    let repos = dir("irepos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("issue", Some(&gh))?;
    let prompt_file = world.root.join("prompt");
    let mut asked = request(Harness::Command(vec![
        "/bin/sh".into(),
        "-c".into(),
        format!("printf '%s' \"$0\" > {}", prompt_file.display()),
    ]));
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
    assert!(
        said.contains("issue #7 read through the broker (Open, "),
        "{said}"
    );
    let prompt = std::fs::read_to_string(&prompt_file)?;
    assert!(
        prompt.contains("written on GitHub by its users"),
        "{prompt}"
    );
    assert!(
        prompt.contains(&format!(
            "<issue number=\"7\">\nTitle: {ISSUE_TITLE}\nLabels: docs\n\n{ISSUE_BODY}\n</issue>"
        )),
        "{prompt}"
    );
    // The engine never asked GitHub itself: the one GraphQL call carried the job's token.
    let seen = gh.seen();
    let reads: Vec<_> = seen.iter().filter(|s| s.path == "/graphql").collect();
    assert_eq!(reads.len(), 1, "{seen:?}");
    assert!(
        reads[0].authorization.starts_with("bearer ghs_"),
        "{seen:?}"
    );
    assert!(gh.issued().is_empty(), "no token is left live");
    // The forge's text is not in the log: the task records a digest of the request.
    let raw = std::fs::read(world.data.join("events.db"))?;
    assert!(!contains(&raw, b"issue-body-marker-5b1e"));

    // No such issue: the job fails, no sandbox runs, the token is revoked.
    let _ = std::fs::remove_file(&prompt_file);
    asked.issue_number = Some(99);
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
        said.contains("issue #99 could not be read (forge.no_such_issue)"),
        "{said}"
    );
    assert!(!said.contains("sandbox"), "{said}");
    assert!(!prompt_file.exists());
    assert!(gh.issued().is_empty(), "no token is left live");
    let events = world.events()?;
    assert!(events.contains(&Event::JobEnded {
        job: summary.job,
        outcome: JobOutcome::Failed
    }));
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
