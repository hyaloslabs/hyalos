//! S8.5: chaos. Each dependency a running job needs is killed or taken down mid-job, and every
//! scenario must end with the task held, never with the job going on unguarded or the task completed
//! (ADR-0013, ADR-0106).
//!
//! `run_job` runs the real engine, policy, broker and GitHub App code against the stand-in GitHub on
//! loopback and a local-process sandbox, as in `run_job.rs`. The sandbox is this test executable,
//! which tells the test when it is running and then does what the scenario needs:
//! - **the broker**: the broker process is killed with `SIGKILL`;
//! - **the sandbox**: the harness process is killed with `SIGKILL`, so its runtime has lost it;
//! - **the model provider**: the provider cannot be reached, and the relay gives up after its retries;
//! - **the forge**: the stand-in GitHub goes down before the harness opens its pull request.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hyalos::run_cmd::{Harness, Request, Summary, run_job};
use hyalos_substrate::{Substrate, choose};
use hyalos_substrate_contract::LocalProcessSubstrate;
use hyalos_types::{Event, Gate, JobOutcome, ReasonCode};

mod common;

use common::{GitHub, Res, World, dir, have_git, repository};

/// How long a scenario may take: the sandbox waits far longer, so a job that ends in time was stopped.
const IN_TIME: Duration = Duration::from_secs(90);

fn local_detect() -> hyalos::run_cmd::Detect {
    Box::new(|image: &str| {
        choose(
            "linux",
            vec![Box::new(LocalProcessSubstrate::new(image)) as Box<dyn Substrate>],
            image,
        )
    })
}

/// One scenario's world, started: a repository, the stand-in GitHub and a job in `mode`.
struct Scenario {
    world: World,
    gh: GitHub,
    out: PathBuf,
    command: Vec<String>,
    _dirs: (PathBuf, PathBuf),
}

impl Scenario {
    fn new(name: &str, mode: &str) -> Res<Self> {
        let repos = dir(&format!("{name}-repos"))?;
        let home = dir(&format!("{name}-home"))?;
        repository(&repos, &home)?;
        let gh = GitHub::start(&repos, false)?;
        let world = World::new(name, Some(&gh))?;
        let out = world.root.join("out");
        std::fs::create_dir(&out)?;
        let command = vec![
            std::env::current_exe()?.to_string_lossy().into_owned(),
            "harness_entry".into(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
            format!("runtime:{}", world.runtime.display()),
            format!("out:{}", out.display()),
            format!("mode:{mode}"),
        ];
        Ok(Self {
            world,
            gh,
            out,
            command,
            _dirs: (repos, home),
        })
    }

    /// Runs the job on a thread and calls `chaos` once the harness says it is running. Returns the
    /// summary, what the run printed and how long it took.
    fn run(&self, chaos: impl FnOnce(&Self) -> Res) -> Res<(Summary, String, Duration)> {
        let request = Request {
            repo: "octo/repo".into(),
            issue: "Add a CHANGES file.".into(),
            issue_number: None,
            base: "main".into(),
            image: "harness".into(),
            harness: Harness::Command(self.command.clone()),
            budget_tokens: 10_000,
            minutes: 5,
            egress_hosts: BTreeSet::new(),
            step_model: None,
            release_issue: false,
            await_release: false,
            nonce: None,
            skills: Default::default(),
            workflow: None,
        };
        let began = Instant::now();
        let setup = self.world.setup(local_detect(), Duration::from_secs(10));
        let (summary, said) = std::thread::scope(|scope| -> Res<_> {
            let job = scope.spawn(move || {
                let mut said = Vec::new();
                let summary = run_job(&request, setup, &mut said).map_err(|e| e.to_string());
                (summary, String::from_utf8_lossy(&said).into_owned())
            });
            wait_for(&self.out.join("ready"))?;
            chaos(self)?;
            job.join().map_err(|_| "the run panicked".into())
        })?;
        Ok((summary?, said, began.elapsed()))
    }

    /// The run ended held by the dependency gate, with `code`, in time, and its task is not completed.
    fn assert_held(&self, summary: &Summary, said: &str, took: Duration, code: &str) -> Res {
        assert_eq!(summary.outcome, JobOutcome::Held, "{said}");
        assert!(took < IN_TIME, "the sandbox was not stopped: {said}");
        let events = self.world.events()?;
        assert!(
            events.contains(&Event::JobEnded {
                job: summary.job,
                outcome: JobOutcome::Held,
                reason: ReasonCode::new(code).ok_or("code")?,
            }),
            "{events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::HoldOpened {
                    gate: Gate::Dependency,
                    ..
                }
            )),
            "{events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::TaskCompleted { .. })),
            "a lost dependency completed the task: {events:?}"
        );
        // Nothing the job did reached the forge: no push, and no pull request opened.
        assert!(
            !self
                .gh
                .seen()
                .iter()
                .any(|s| s.path.ends_with("/git-receive-pack")),
            "the job pushed"
        );
        assert!(
            !self.out.join("pulled").exists(),
            "a pull request was opened"
        );
        Ok(())
    }
}

fn wait_for(path: &Path) -> Res {
    let began = Instant::now();
    while !path.exists() {
        if began.elapsed() > Duration::from_secs(60) {
            return Err(format!("{} never appeared", path.display()).into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn kill(pid: &str) -> Res {
    let status = std::process::Command::new("kill")
        .args(["-KILL", pid])
        .status()?;
    if !status.success() {
        return Err(format!("could not kill {pid}").into());
    }
    Ok(())
}

/// The broker this world's run started: `hyalos broker <socket> <provider keys>`, where only this
/// world has these provider keys, so a scenario never kills another's broker.
fn broker_of(world: &World) -> Res<String> {
    let keys = world.config.join("provider.keys");
    let keys = keys.to_string_lossy();
    for entry in std::fs::read_dir("/proc")?.filter_map(Result::ok) {
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = cmdline
            .split(|b| *b == 0)
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if args.get(1).map(String::as_str) == Some("broker") && args.iter().any(|a| *a == *keys) {
            return Ok(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Err("no broker for this world".into())
}

#[test]
fn a_broker_killed_mid_job_holds_the_task() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let s = Scenario::new("chaos-broker", "wait")?;
    let (summary, said, took) = s.run(|s| kill(&broker_of(&s.world)?))?;
    s.assert_held(&summary, &said, took, "broker.lost")?;
    assert!(said.contains("the broker was lost"), "{said}");
    Ok(())
}

#[test]
fn a_sandbox_its_runtime_lost_mid_job_holds_the_task() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let s = Scenario::new("chaos-sandbox", "wait")?;
    let (summary, said, took) = s.run(|s| {
        let pid = std::fs::read_to_string(s.out.join("pid"))?;
        kill(pid.trim())
    })?;
    s.assert_held(&summary, &said, took, "sandbox.lost")?;
    Ok(())
}

#[test]
fn a_model_provider_lost_mid_job_holds_the_task() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    // The world's provider base is a port nothing listens on, so every attempt is unreachable and
    // the relay gives up after its retries.
    let s = Scenario::new("chaos-model", "model")?;
    let (summary, said, took) = s.run(|_| Ok(()))?;
    s.assert_held(&summary, &said, took, "model.lost")?;
    // The harness was refused, never answered by a provider.
    let answer = std::fs::read_to_string(s.out.join("attempted"))?;
    assert!(!answer.contains(" 200 "), "{answer}");
    Ok(())
}

#[test]
fn a_forge_lost_mid_job_holds_the_task() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let s = Scenario::new("chaos-forge", "pull")?;
    let (summary, said, took) = s.run(|s| {
        s.gh.go_down();
        std::fs::write(s.out.join("go"), "")?;
        // Back up once the harness was refused, so the token can still be revoked.
        wait_for(&s.out.join("attempted"))?;
        s.gh.come_up();
        Ok(())
    })?;
    s.assert_held(&summary, &said, took, "forge.lost")?;
    let answer = std::fs::read_to_string(s.out.join("attempted"))?;
    assert!(!answer.contains(" 201 "), "{answer}");
    Ok(())
}

/// The sandbox's harness: this executable, re-entered. It says it is running, does what its mode
/// says, and then waits far longer than any scenario, so only a stop ends it in time.
#[test]
fn harness_entry() -> Res {
    let arg =
        |prefix: &str| std::env::args().find_map(|a| a.strip_prefix(prefix).map(PathBuf::from));
    let (Some(runtime), Some(out), Some(mode)) = (arg("runtime:"), arg("out:"), arg("mode:"))
    else {
        return Ok(());
    };
    let socket = std::fs::read_dir(&runtime)?
        .filter_map(Result::ok)
        .map(|e| e.path().join("broker.sock"))
        .find(|p| p.exists())
        .ok_or("no job socket")?;
    let port = forwarder(&socket)?;
    std::fs::write(out.join("pid"), std::process::id().to_string())?;
    std::fs::write(out.join("ready"), "")?;
    if mode == Path::new("model") {
        let body = br#"{"model":"gpt-5","input":"plan the work"}"#;
        let (status, _) = call(
            port,
            "POST /v1/responses HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n",
            body,
        )?;
        std::fs::write(out.join("attempted"), status)?;
    }
    if mode == Path::new("pull") {
        wait_for(&out.join("go"))?;
        let branch = std::env::var("HYALOS_BRANCH")?;
        let pull = serde_json::json!({
            "title": "Add CHANGES", "body": "Adds it.", "head": branch, "base": "main",
        })
        .to_string();
        let (status, _) = call(
            port,
            "POST /forge/repos/octo/repo/pulls HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n",
            pull.as_bytes(),
        )?;
        if status.contains(" 201 ") {
            std::fs::write(out.join("pulled"), "")?;
        }
        std::fs::write(out.join("attempted"), status)?;
    }
    std::thread::sleep(Duration::from_secs(600));
    Ok(())
}

/// A loopback port that forwards to the job's socket, as the sandbox's forwarder does.
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
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
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
