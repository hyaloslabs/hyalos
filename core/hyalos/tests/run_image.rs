//! S3.10 with a real harness: `hyalos run` starts Codex in the sandbox image under the container
//! substrate, and Codex pushes a branch and opens a pull request (ADR-0050).
//!
//! The model is scripted: Codex's first call is answered with one shell command that clones through
//! the broker, commits, pushes the job's branch, opens the pull request with `hyalos-pr`, and asks
//! curl for a host the manifest does not list. When Codex sends back the command's output, the model
//! holds its answer while the test dumps the running sandbox from outside, as root with every
//! capability: each process's environment and readable memory, and the files under `/tmp`. The
//! bytes are scanned on the host for the provider key and the forge token, which never enter the
//! sandbox, so the scan cannot put them there. Then the model lets Codex finish.
//!
//! S6.3 with the same harness: a skill that is an MCP server runs in the job's sandbox, and its only
//! way out is the egress gate, for the hosts the job and the skill's grant both allow (ADR-0101).
//!
//! It skips without the `hyalos-sandbox:test` image unless `HYALOS_REQUIRE_SANDBOX_IMAGE` is set,
//! which the `sandbox image` workflow does.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

mod common;

use common::{GitHub, Res, World, codex, dir, git, have_git, ok, ok_cli, repository};
use hyalos_types::{Answer, CredentialTier, Event, Gate, JobOutcome};

const IMAGE: &str = "hyalos-sandbox:test";
/// How long the test may take to dump the sandbox while the model holds its answer.
const HOLD: Duration = Duration::from_secs(180);

/// The container runtime that has `image`, or `None` to skip.
fn runtime() -> Res<Option<&'static str>> {
    for runtime in ["docker", "podman"] {
        let found = Command::new(runtime)
            .args(["image", "inspect", IMAGE])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if found {
            return Ok(Some(runtime));
        }
    }
    if std::env::var_os("HYALOS_REQUIRE_SANDBOX_IMAGE").is_some() {
        return Err(format!("HYALOS_REQUIRE_SANDBOX_IMAGE is set and there is no {IMAGE}").into());
    }
    Ok(None)
}

/// The one command Codex is told to run.
const SCRIPT: &str = r#"set -e
git clone "$HYALOS_REMOTE" /tmp/work
cd /tmp/work
git checkout -b "$HYALOS_BRANCH"
echo 'Added.' > CHANGES
git add CHANGES
git commit -m 'add CHANGES'
git push origin "$HYALOS_BRANCH"
printf 'Closes the issue.\n' > /tmp/pr.md
hyalos-pr "$HYALOS_REPO" "$HYALOS_BRANCH" "$HYALOS_BASE" 'Add a CHANGES file' /tmp/pr.md
curl -sS --max-time 15 https://unlisted.example.com/ || echo 'egress refused'
"#;

/// A scripted OpenAI Responses model. It answers the first call that offers `exec_command` with
/// [`SCRIPT`]; when the command's output comes back it says so on `held` and waits for `go` before
/// answering, so the sandbox can be dumped while Codex is running.
struct Model {
    port: u16,
    calls: Arc<Mutex<usize>>,
}

fn model(held: mpsc::Sender<()>, go: mpsc::Receiver<()>) -> Res<Model> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let calls = Arc::new(Mutex::new(0));
    let counted = calls.clone();
    let go = Arc::new(Mutex::new(Some(go)));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let (held, go, counted) = (held.clone(), go.clone(), counted.clone());
            std::thread::spawn(move || {
                let _ = answer(stream, &held, &go, &counted);
            });
        }
    });
    Ok(Model { port, calls })
}

fn answer(
    stream: TcpStream,
    held: &mpsc::Sender<()>,
    go: &Mutex<Option<mpsc::Receiver<()>>>,
    calls: &Mutex<usize>,
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
    let answered = body.to_string().contains("\"function_call_output\"");
    let reply = if offered && !answered {
        codex::run_command(SCRIPT, stream_wanted)
    } else {
        if answered {
            // The command has run and Codex is waiting: hold here while the sandbox is dumped.
            let waiting = go.lock().ok().and_then(|mut g| g.take());
            if let Some(waiting) = waiting {
                let _ = held.send(());
                let _ = waiting.recv_timeout(HOLD);
            }
        }
        codex::done(stream_wanted)
    };
    let content_type = if stream_wanted {
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

/// Reads every readable mapping of every other process, written to standard output. Run in the
/// sandbox by `exec` as root with every capability, which may read any process's memory.
const DUMP_MEMORY: &str = r#"
const fs = require('fs');
const out = (b) => { let o = 0; while (o < b.length) { try { o += fs.writeSync(1, b, o); } catch (e) { if (e.code !== 'EAGAIN') throw e; } } };
const buf = Buffer.alloc(1 << 20);
for (const pid of fs.readdirSync('/proc').filter((p) => /^\d+$/.test(p))) {
  if (Number(pid) === process.pid) continue;
  let maps, fd;
  try { maps = fs.readFileSync(`/proc/${pid}/maps`, 'utf8'); fd = fs.openSync(`/proc/${pid}/mem`, 'r'); } catch { continue; }
  for (const line of maps.split('\n')) {
    const [range, perms] = line.split(' ');
    if (!perms || perms[0] !== 'r' || line.includes('[vvar') || line.includes('[vsyscall]')) continue;
    const [start, end] = range.split('-').map((x) => BigInt('0x' + x));
    for (let at = start; at < end; ) {
      const want = Number(end - at < BigInt(buf.length) ? end - at : BigInt(buf.length));
      let got;
      try { got = fs.readSync(fd, buf, 0, want, at); } catch { break; }
      if (got <= 0) break;
      out(buf.subarray(0, got));
      at += BigInt(got);
    }
  }
  fs.closeSync(fd);
}
"#;

/// Runs `args` in the job's container as root with every capability and counts, without keeping
/// the output, how often each needle appears in it. Returns the counts and the bytes read.
fn scan(
    runtime: &str,
    container: &str,
    args: &[&str],
    needles: &[&[u8]],
) -> Res<(Vec<usize>, usize)> {
    let mut child = Command::new(runtime)
        .args(["exec", "-i", "--privileged", "--user", "0", container])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or("no output")?;
    let longest = needles.iter().map(|n| n.len()).max().unwrap_or(0);
    let mut counts = vec![0; needles.len()];
    let (mut carry, mut total, mut chunk) = (Vec::new(), 0usize, vec![0u8; 1 << 20]);
    loop {
        let n = stdout.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        total += n;
        // The tail of the last chunk, so a needle split across two reads is still found once.
        let keep = carry.len();
        carry.extend_from_slice(&chunk[..n]);
        for (needle, count) in needles.iter().zip(counts.iter_mut()) {
            *count += carry
                .windows(needle.len())
                .enumerate()
                .filter(|(i, w)| w == needle && i + needle.len() > keep)
                .count();
        }
        let tail = carry.len().saturating_sub(longest.saturating_sub(1));
        carry.drain(..tail);
    }
    child.wait()?;
    Ok((counts, total))
}

#[test]
fn codex_in_the_sandbox_pushes_a_branch_and_opens_a_pull_request() -> Res {
    let Some(runtime) = runtime()? else {
        eprintln!("skipped: no {IMAGE} image");
        return Ok(());
    };
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let repos = dir("irepos")?;
    let home = dir("ihome")?;
    repository(&repos, &home)?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("image", Some(&gh))?;
    let (held_tx, held) = mpsc::channel();
    let (go, go_rx) = mpsc::channel();
    let model = model(held_tx, go_rx)?;
    std::fs::write(
        world.config.join("provider.keys"),
        format!(
            "openai.key = {}\nopenai.base = http://127.0.0.1:{}\n",
            world.provider_key, model.port
        ),
    )?;
    let job = world.events()?.len() as u64 + 2;

    let mut run = Command::new(common::EXE)
        .args([
            "run",
            "--repo",
            "octo/repo",
            "--issue",
            "Add a CHANGES file.",
        ])
        .args(["--image", IMAGE, "--harness", "codex", "--minutes", "5"])
        // A listed host declares the job's web flow, so the unlisted one curl asks for is the egress
        // gate's refusal (ADR-0055).
        .args(["--allow-host", "listed.example"])
        .env("HYALOS_CONFIG_DIR", &world.config)
        .env("HYALOS_DATA_DIR", &world.data)
        .env("HYALOS_RUNTIME_DIR", &world.runtime)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = run.stdout.take().ok_or("no stdout")?;
    let mut stderr = run.stderr.take().ok_or("no stderr")?;
    let said = std::thread::spawn(move || {
        let (mut out, mut err) = (String::new(), String::new());
        let _ = stdout.read_to_string(&mut out);
        let _ = stderr.read_to_string(&mut err);
        format!("{out}{err}")
    });

    // While Codex waits for the model, dump the sandbox from outside.
    let dumped = held.recv_timeout(Duration::from_secs(240)).is_ok();
    let container = if dumped {
        let listed = Command::new(runtime)
            .args(["ps", "-q", "--no-trunc", "--filter"])
            .arg(format!("label=hyalos.job={job}"))
            .output()?;
        String::from_utf8_lossy(&listed.stdout).trim().to_string()
    } else {
        String::new()
    };
    let token = gh.issued().last().cloned().unwrap_or_default();
    let scans = if container.is_empty() {
        Vec::new()
    } else {
        let secrets: [&[u8]; 3] = [
            token.as_bytes(),
            world.provider_key.as_bytes(),
            // A control: what the job was given, so the scan is shown to read the harness.
            b"hyalos/job-",
        ];
        vec![
            (
                "environments",
                scan(
                    runtime,
                    &container,
                    &["sh", "-c", "cat /proc/[0-9]*/environ"],
                    &secrets,
                )?,
            ),
            (
                "memory",
                scan(runtime, &container, &["node", "-e", DUMP_MEMORY], &secrets)?,
            ),
            (
                "files",
                scan(
                    runtime,
                    &container,
                    &["tar", "-C", "/", "-cf", "-", "tmp"],
                    &secrets,
                )?,
            ),
        ]
    };
    let _ = go.send(());
    let status = run.wait()?;
    let said = said.join().map_err(|_| "reader panicked")?;
    assert!(dumped, "Codex never sent the command's output back: {said}");
    assert!(!container.is_empty(), "no container for job {job}: {said}");
    assert!(!token.is_empty(), "a token was minted");
    for (what, (counts, bytes)) in &scans {
        eprintln!(
            "scanned the sandbox's {what}: {bytes} bytes, the control found {} times",
            counts[2]
        );
        assert_eq!(counts[0], 0, "the forge token is in the sandbox's {what}");
        assert_eq!(counts[1], 0, "the provider key is in the sandbox's {what}");
        if *what != "files" {
            assert!(
                counts[2] > 0,
                "the scan of {what} read the harness ({bytes} bytes)"
            );
        }
    }
    let memory = scans.iter().find(|(w, _)| *w == "memory").ok_or("memory")?;
    assert!(
        memory.1.1 > 50 * 1024 * 1024,
        "a real memory dump: {} bytes",
        memory.1.1
    );
    assert!(status.success(), "{said}");
    assert!(*model.calls.lock().map_err(|_| "lock")? >= 2);

    // The branch, the pull request and the revoke at the stand-in.
    let branch = format!("hyalos/job-{job}");
    let log = ok(git(
        &home,
        &repos.join("octo/repo.git"),
        &["log", "--format=%s", &branch],
    )?)?;
    assert!(log.starts_with("add CHANGES\n"), "{log}\n{said}");
    let seen = gh.seen();
    let pull = seen
        .iter()
        .find(|s| s.method == "POST" && s.path == "/repos/octo/repo/pulls")
        .ok_or("no pull request")?;
    let pull: Value = serde_json::from_slice(&pull.body)?;
    assert_eq!(
        (pull["head"].as_str(), pull["base"].as_str()),
        (Some(branch.as_str()), Some("main"))
    );
    assert!(
        seen.iter()
            .any(|s| s.method == "DELETE" && s.path == "/installation/token")
    );
    assert!(gh.issued().is_empty(), "no token is left live");

    // The log: the substrate detection chose on this runtime, gVisor where it is set up (ADR-0056),
    // broker-held credentials, and curl's refused host.
    let events = world.events()?;
    let ran_on = events.iter().find_map(|e| match e {
        Event::JobStarted {
            substrate,
            credentials: CredentialTier::BrokerHeld,
            ..
        } => Some(substrate.as_str().to_string()),
        _ => None,
    });
    eprintln!("the job ran on {ran_on:?}");
    assert!(
        ran_on.as_deref() == Some(&format!("container.{runtime}"))
            || ran_on.as_deref() == Some(&format!("gvisor.{runtime}")),
        "{events:?}"
    );
    if std::env::var_os("HYALOS_REQUIRE_GVISOR").is_some() {
        assert_eq!(ran_on, Some(format!("gvisor.{runtime}")));
    }
    assert!(
        events.iter().any(|e| matches!(e,
        Event::GateAnswered { gate: Gate::Egress, answer: Answer::Deny, reason, .. }
            if reason.as_str() == "egress.host_not_allowed")),
        "{said}"
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::JobEnded {
            outcome: JobOutcome::Succeeded,
            ..
        }
    )));
    ok_cli(world.cli(&["log", "verify"])?)?;
    let _ = std::fs::remove_dir_all(&repos);
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

/// A model that answers every call with "Done.", so nothing in the sandbox asks for a host but the
/// MCP server.
fn quiet_model() -> Res<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(&stream);
                let (mut line, mut length) = (String::new(), 0usize);
                let _ = reader.read_line(&mut line);
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let stream_wanted = body["stream"].as_bool().unwrap_or(false);
                let reply = codex::done(stream_wanted);
                let kind = if stream_wanted {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                let _ = write!(
                    &stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            });
        }
    });
    Ok(port)
}

/// The MCP server's start: it asks twice for the host its grant holds and once for one the job lists
/// but the grant cut, then ends. Codex starts it before its first turn.
const SERVE: &str = "#!/bin/sh\n\
curl -sS --max-time 10 -o /dev/null https://granted.example/ 2>/dev/null\n\
curl -sS --max-time 10 -o /dev/null https://granted.example/ 2>/dev/null\n\
curl -sS --max-time 10 -o /dev/null https://cut.example/ 2>/dev/null\n\
exit 0\n";

#[test]
fn an_mcp_server_in_the_sandbox_reaches_only_what_its_grant_allows() -> Res {
    let Some(_runtime) = runtime()? else {
        eprintln!("skipped: no {IMAGE} image");
        return Ok(());
    };
    if !have_git()? {
        eprintln!("skipped: no git");
        return Ok(());
    }
    let repos = dir("mrepos")?;
    let home = dir("mhome")?;
    repository(&repos, &home)?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("image-mcp", Some(&gh))?;
    let port = quiet_model()?;
    std::fs::write(
        world.config.join("provider.keys"),
        format!(
            "openai.key = {}\nopenai.base = http://127.0.0.1:{port}\n",
            world.provider_key
        ),
    )?;

    // The skill: an MCP server granted one host, and what the job needs to run.
    let skill = world.root.join("notes");
    std::fs::create_dir_all(&skill)?;
    std::fs::write(skill.join("SKILL.md"), "---\nname: notes\n---\nNotes.\n")?;
    std::fs::write(skill.join("serve"), SERVE)?;
    std::fs::set_permissions(
        skill.join("serve"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )?;
    std::fs::write(skill.join("mcp.json"), r#"{"command":"./serve"}"#)?;
    std::fs::write(
        skill.join("capabilities.json"),
        r#"{"version":1,"actions":["read","push","open_pr"],"egress_hosts":["granted.example"],
            "data_flows":["job->model","job->forge","job->web"]}"#,
    )?;
    let path = skill.to_string_lossy().into_owned();
    let out = ok_cli(world.signed(&["plugin", "install", &path, "--grant"])?)?;
    assert!(out.contains("runs an MCP server: ./serve"), "{out}");

    // The job lists both hosts; the skill's grant cuts it to one.
    let run = Command::new(common::EXE)
        .args(["run", "--repo", "octo/repo", "--issue", "Take notes."])
        .args(["--image", IMAGE, "--harness", "codex", "--minutes", "5"])
        .args([
            "--allow-host",
            "granted.example",
            "--allow-host",
            "cut.example",
        ])
        .args(["--skill", "notes"])
        .env("HYALOS_CONFIG_DIR", &world.config)
        .env("HYALOS_DATA_DIR", &world.data)
        .env("HYALOS_RUNTIME_DIR", &world.runtime)
        .output()?;
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let egress: Vec<(Answer, String)> = world
        .events()?
        .into_iter()
        .filter_map(|e| match e {
            Event::GateAnswered {
                gate: Gate::Egress,
                answer,
                reason,
                ..
            } => Some((answer, reason.as_str().to_string())),
            _ => None,
        })
        .collect();
    eprintln!("egress answers: {egress:?}");
    // The server ran in the sandbox and its requests reached the gate. A refusal is logged and an
    // allow is not (ADR-0042), so exactly one refusal means the host the grant cut was refused and
    // the granted host, asked for twice, was not.
    assert_eq!(
        egress,
        [(Answer::Deny, "egress.host_not_allowed".to_string())],
        "{said}"
    );
    ok_cli(world.cli(&["log", "verify"])?)?;
    let _ = std::fs::remove_dir_all(&repos);
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
