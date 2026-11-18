//! S8.8: a release runs one complete job with the binary it is about to publish (ADR-0107).
//!
//! The release workflow builds the static `hyalos`, then runs this test with
//! `HYALOS_RELEASE_JOB_BINARY` naming that binary and `HYALOS_RELEASE_JOB_IMAGE` the sandbox image it
//! names by default. There is no secret in it: the model is scripted and GitHub is the stand-in the
//! other tests use. One run of the fix-issue workflow:
//! - starts the task;
//! - reads issue 7 as tainted in a reader, and plans from its typed values;
//! - runs Codex as the actor, in the image the binary names, not one given on the command line;
//! - pushes the job's branch, opens a pull request and submits its result.
//!
//! Then the binary's own `hyalos log verify` checks the log. Without `HYALOS_RELEASE_JOB_BINARY` the
//! run is skipped; the release workflow always sets it.
//!
//! The second test shows the check catches a binary that refuses every job, as v0.1.0's did (#280):
//! a stand-in that prints a refusal and exits 0 must fail it. It needs no image, so it runs in CI.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

mod common;

use common::{GitHub, Res, World, codex, dir, git, have_git, ok, ok_cli, repository};
use hyalos_types::{Event, JobOutcome};

/// What Codex is told to run: the actor's whole job.
const SCRIPT: &str = r#"set -e
git clone "$HYALOS_REMOTE" /tmp/work
cd /tmp/work
git checkout -b "$HYALOS_BRANCH"
echo 'Added.' > CHANGES
git add CHANGES
git commit -m 'add CHANGES'
git push origin "$HYALOS_BRANCH"
printf 'Adds a CHANGES file.\n' > /tmp/pr.md
hyalos-pr "$HYALOS_REPO" "$HYALOS_BRANCH" "$HYALOS_BASE" 'Add a CHANGES file' /tmp/pr.md
hyalos-submit '{"outcome":"pr_opened","tests_added":false}'
"#;

/// The reader's and the planner's answers, in order.
const STEPS: [&str; 2] = [
    r#"{"kind": "docs", "size": "small", "clear": true, "has_repro": false}"#,
    "```json\n{\"approach\": \"docs_only\", \"add_test\": false}\n```",
];

/// How long the whole job may take, the image's start included.
const IN_TIME: Duration = Duration::from_secs(420);

#[test]
fn the_release_binary_runs_a_complete_job() -> Res {
    let Some(binary) = std::env::var_os("HYALOS_RELEASE_JOB_BINARY").map(PathBuf::from) else {
        eprintln!("skipped: HYALOS_RELEASE_JOB_BINARY is not set");
        return Ok(());
    };
    let image = std::env::var("HYALOS_RELEASE_JOB_IMAGE")
        .map_err(|_| "HYALOS_RELEASE_JOB_IMAGE names the image the binary runs")?;
    let said = complete_job("rel", &binary, Some(&image))?;
    eprintln!("{said}");
    Ok(())
}

#[test]
fn a_binary_that_refuses_every_job_fails_the_check() -> Res {
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    // As v0.1.0 did: every other command works, and `run` is refused but exits 0, so only what the
    // job left behind can tell.
    let scratch = dir("refuser")?;
    let refuser = scratch.join("hyalos");
    std::fs::write(
        &refuser,
        format!(
            "#!/bin/sh\nif [ \"$1\" = run ]; then\n  echo 'job 1: no manifest: policy.unknown'\n  exit 0\nfi\nexec {} \"$@\"\n",
            common::EXE
        ),
    )?;
    std::fs::set_permissions(
        &refuser,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )?;
    let checked = complete_job("ref", &refuser, None);
    let _ = std::fs::remove_dir_all(&scratch);
    let Err(why) = checked else {
        return Err("a binary that refuses every job passed the check".into());
    };
    eprintln!("refused as it should be: {why}");
    Ok(())
}

/// Runs the fix-issue workflow on issue 7 with `binary`, in directories named for `name`, and checks everything a complete job leaves
/// behind. `image` is the sandbox image the binary should run by default, checked against the
/// running container; `None` skips that check for a binary that never starts one. `Err` says what
/// is missing; `Ok` is what the run printed.
fn complete_job(name: &str, binary: &Path, image: Option<&str>) -> Res<String> {
    if !have_git()? {
        return Err("no git".into());
    }
    let repos = dir(&format!("{name}-repos"))?;
    let home = dir(&format!("{name}-home"))?;
    repository(&repos, &home)?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new(name, Some(&gh))?;
    // The starter policy is what a new operator approves (S5.5).
    let starter = world.root.join("starter.cedar");
    std::fs::write(&starter, ok_cli(world.cli(&["policy", "starter"])?)?)?;
    ok_cli(world.signed(&["policy", "approve", &starter.to_string_lossy()])?)?;
    let (held_tx, held) = mpsc::channel();
    let (go, go_rx) = mpsc::channel();
    let model = Scripted::start(held_tx, go_rx)?;
    std::fs::write(
        world.config.join("provider.keys"),
        format!(
            "openai.key = {}\nopenai.base = http://127.0.0.1:{}\n",
            world.provider_key, model.port
        ),
    )?;

    let began = Instant::now();
    let mut run = Command::new(binary)
        .args(["run", "--repo", "octo/repo", "--issue-number", "7"])
        .args(["--issue", "Add a CHANGES file.", "--harness", "codex"])
        .args(["--minutes", "5"])
        .env("HYALOS_CONFIG_DIR", &world.config)
        .env("HYALOS_DATA_DIR", &world.data)
        .env("HYALOS_RUNTIME_DIR", &world.runtime)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = run.stdout.take().ok_or("no stdout")?;
    let mut stderr = run.stderr.take().ok_or("no stderr")?;
    let reading = std::thread::spawn(move || {
        let (mut out, mut err) = (String::new(), String::new());
        let _ = stdout.read_to_string(&mut out);
        let _ = stderr.read_to_string(&mut err);
        format!("{out}{err}")
    });

    // While Codex waits for its last answer, the container it runs in is looked at from outside.
    let mut ran_in = None;
    loop {
        if held.recv_timeout(Duration::from_millis(500)).is_ok() {
            ran_in = Some(images_running()?);
            let _ = go.send(());
            break;
        }
        if run.try_wait()?.is_some() || began.elapsed() > IN_TIME {
            break;
        }
    }
    let status = loop {
        if let Some(status) = run.try_wait()? {
            break status;
        }
        if began.elapsed() > IN_TIME {
            let _ = run.kill();
            break run.wait()?;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let said = reading.join().map_err(|_| "the reader panicked")?;
    let checked = check(&world, &gh, &home, &repos, binary, image, ran_in, &said);
    let _ = std::fs::remove_dir_all(&repos);
    let _ = std::fs::remove_dir_all(&home);
    checked?;
    if !status.success() {
        return Err(format!("hyalos run exited {status}: {said}").into());
    }
    if began.elapsed() > IN_TIME {
        return Err(format!("the job took longer than {IN_TIME:?}: {said}").into());
    }
    if model.calls() < STEPS.len() + 2 {
        return Err(format!("the model was called {} times: {said}", model.calls()).into());
    }
    Ok(said)
}

/// Everything a complete job leaves behind.
#[allow(clippy::too_many_arguments)]
fn check(
    world: &World,
    gh: &GitHub,
    home: &Path,
    repos: &Path,
    binary: &Path,
    image: Option<&str>,
    ran_in: Option<Vec<String>>,
    said: &str,
) -> Res {
    let fail = |what: &str| -> Res { Err(format!("{what}: {said}").into()) };
    // The workflow: three steps, the reader's and the planner's typed values.
    if !said.contains("workflow fix_issue: 3 step(s)") {
        return fail("the fix-issue workflow did not start");
    }
    if !said.contains("step triage answered: clear=true, has_repro=false, kind=docs, size=small") {
        return fail("the reader did not answer");
    }
    if !said.contains("step plan answered: add_test=false, approach=docs_only") {
        return fail("the planner did not answer");
    }
    // The actor ran in the image the binary names by default.
    if let Some(image) = image {
        let ran_in = ran_in.ok_or("Codex never asked for its last answer")?;
        if !ran_in.iter().any(|i| i == image) {
            return Err(format!("the actor ran in {ran_in:?}, not {image}: {said}").into());
        }
    }
    // The branch and the pull request at the stand-in, and the token revoked.
    let seen = gh.seen();
    let pull = seen
        .iter()
        .find(|s| s.method == "POST" && s.path == "/repos/octo/repo/pulls")
        .ok_or_else(|| format!("no pull request was opened: {said}"))?;
    let pull: Value = serde_json::from_slice(&pull.body)?;
    let branch = pull["head"].as_str().unwrap_or_default();
    if !branch.starts_with("hyalos/job-") {
        return fail("the pull request is not from a job's branch");
    }
    let log = git(
        home,
        &repos.join("octo/repo.git"),
        &["log", "--format=%s", branch],
    )
    .and_then(ok)
    .unwrap_or_default();
    if !log.starts_with("add CHANGES\n") {
        return fail(&format!("the branch {branch} was not pushed"));
    }
    if !gh.issued().is_empty() {
        return fail("a token is left live");
    }
    // The log: every job succeeded and the task completed, the reader read the issue as tainted.
    let events = world.events()?;
    let ended: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::JobEnded { outcome, .. } => Some(*outcome),
            _ => None,
        })
        .collect();
    if ended != [JobOutcome::Succeeded; 3] {
        return Err(format!("the jobs ended {ended:?}: {said}").into());
    }
    if !events
        .iter()
        .any(|e| matches!(e, Event::TaskCompleted { .. }))
    {
        return fail("the task was not completed");
    }
    // The binary's own verify, on the log it wrote.
    let verified = Command::new(binary)
        .args(["log", "verify"])
        .env("HYALOS_CONFIG_DIR", &world.config)
        .env("HYALOS_DATA_DIR", &world.data)
        .env("HYALOS_RUNTIME_DIR", &world.runtime)
        .output()?;
    ok_cli(verified).map_err(|e| format!("hyalos log verify: {e}"))?;
    Ok(())
}

/// The images of the running containers labelled as Hyalos jobs.
fn images_running() -> Res<Vec<String>> {
    let listed = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "label=hyalos.job",
            "--format",
            "{{.Image}}",
        ])
        .output()?;
    Ok(String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

/// The scripted model: the reader's and the planner's answers in order, then Codex's two turns. On
/// Codex's last call it says so on `held` and waits for `go`, so the container can be looked at.
struct Scripted {
    port: u16,
    calls: Arc<Mutex<usize>>,
}

impl Scripted {
    fn start(held: mpsc::Sender<()>, go: mpsc::Receiver<()>) -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let calls = Arc::new(Mutex::new(0));
        let counted = calls.clone();
        let go = Arc::new(Mutex::new(Some(go)));
        let steps = Arc::new(Mutex::new(STEPS.iter().rev().copied().collect::<Vec<_>>()));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (held, go, counted, steps) =
                    (held.clone(), go.clone(), counted.clone(), steps.clone());
                std::thread::spawn(move || {
                    let _ = answer(stream, &held, &go, &counted, &steps);
                });
            }
        });
        Ok(Self { port, calls })
    }

    fn calls(&self) -> usize {
        self.calls.lock().map(|c| *c).unwrap_or(0)
    }
}

fn answer(
    stream: TcpStream,
    held: &mpsc::Sender<()>,
    go: &Mutex<Option<mpsc::Receiver<()>>>,
    calls: &Mutex<usize>,
    steps: &Mutex<Vec<&str>>,
) -> Res {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse()?;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    if let Ok(mut calls) = calls.lock() {
        *calls += 1;
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let stream_wanted = body["stream"].as_bool().unwrap_or(false);
    let offered = body["tools"]
        .as_array()
        .is_some_and(|t| t.iter().any(|t| t["name"] == "exec_command"));
    let reply = if !offered {
        // A reader or planner: no tools, one typed answer.
        let text = steps.lock().ok().and_then(|mut s| s.pop()).unwrap_or("{}");
        codex::response(
            "completed",
            serde_json::json!([{
                "type": "message", "id": "msg_step", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": text, "annotations": []}]
            }]),
        )
        .to_string()
    } else if body.to_string().contains("\"function_call_output\"") {
        let waiting = go.lock().ok().and_then(|mut g| g.take());
        if let Some(waiting) = waiting {
            let _ = held.send(());
            let _ = waiting.recv_timeout(Duration::from_secs(60));
        }
        codex::done(stream_wanted)
    } else {
        codex::run_command(SCRIPT, stream_wanted)
    };
    let content_type = if stream_wanted && offered {
        "text/event-stream"
    } else {
        "application/json"
    };
    write!(
        &stream,
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
        reply.len()
    )?;
    Ok(())
}
