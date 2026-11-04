//! S2.9: each harness in the sandbox image makes no lookup other than its model calls, which go
//! through the forwarder to the broker (ADR-0016, ADR-0040).
//!
//! It needs the image's `test` target, built from the repository root:
//!
//!     docker build -f images/sandbox/Containerfile --target test -t hyalos-sandbox:test .
//!
//! It skips if that image is missing, unless `HYALOS_REQUIRE_SANDBOX_IMAGE` is set (the
//! `sandbox image` CI workflow sets it). The broker is the real one, run in this process, with both
//! providers pointed at a scripted model on loopback, so no key and no model is involved. Each
//! harness runs with the container substrate's isolation (no network, read-only root, an empty
//! `/tmp`, an unprivileged user, no capabilities) under `hyalos-dns-log`, which answers DNS on the
//! sandbox's loopback and lists every name asked.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use hyalos_broker::Broker;
use hyalos_broker::model::ThreadSleep;
use hyalos_broker::sandbox::{Models, Providers, Sandboxes};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Budget, Clock, JobId, MANIFEST_VERSION, Manifest, ManifestKey, Role, Timestamp,
};
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const IMAGE: &str = "hyalos-sandbox:test";

struct Wall;

impl Clock for Wall {
    fn now(&self) -> Timestamp {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        Timestamp::from_millis(u64::try_from(ms).unwrap_or(u64::MAX))
    }
}

/// Whether to run: the image is there, or it is required and its absence is a failure.
fn image_present() -> Res<bool> {
    let present = Command::new("docker")
        .args(["image", "inspect", IMAGE])
        .output()
        .is_ok_and(|o| o.status.success());
    if !present && std::env::var_os("HYALOS_REQUIRE_SANDBOX_IMAGE").is_some() {
        return Err(format!("{IMAGE} is required and missing").into());
    }
    if !present {
        eprintln!("skipped: {IMAGE} is not built");
    }
    Ok(present)
}

// ---- a scripted model ----------------------------------------------------------------------

/// Answers every model call with a short text and records the paths it was asked.
fn scripted_model() -> Res<(u16, Arc<Mutex<Vec<String>>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&calls);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
            std::thread::spawn(move || {
                let _ = answer(stream, &seen);
            });
        }
    });
    Ok((port, calls))
}

fn answer(stream: TcpStream, seen: &Mutex<Vec<String>>) -> Res {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line.split(' ').nth(1).unwrap_or_default().to_string();
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
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if let Ok(mut seen) = seen.lock() {
        seen.push(path.clone());
    }
    let stream_wanted = body["stream"].as_bool().unwrap_or(false);
    let model = body["model"].as_str().unwrap_or("scripted").to_string();
    let (content_type, reply) = if path.contains("count_tokens") {
        (
            "application/json",
            json!({ "input_tokens": 10 }).to_string(),
        )
    } else if path.starts_with("/v1/messages") {
        anthropic(&model, stream_wanted)
    } else {
        responses(&model, stream_wanted)
    };
    write!(
        &stream,
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
        reply.len()
    )?;
    Ok(())
}

fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("")))
        .collect()
}

fn anthropic(model: &str, stream: bool) -> (&'static str, String) {
    let usage = json!({ "input_tokens": 10, "output_tokens": 2 });
    if !stream {
        let message = json!({
            "id": "msg_scripted", "type": "message", "role": "assistant", "model": model,
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn", "stop_sequence": null, "usage": usage
        });
        return ("application/json", message.to_string());
    }
    let start = json!({
        "id": "msg_scripted", "type": "message", "role": "assistant", "model": model,
        "content": [], "stop_reason": null, "stop_sequence": null, "usage": usage
    });
    let events = sse(&[
        json!({ "type": "message_start", "message": start }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "ok" } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": 2 } }),
        json!({ "type": "message_stop" }),
    ]);
    ("text/event-stream", events)
}

fn responses(model: &str, stream: bool) -> (&'static str, String) {
    let message = json!({
        "type": "message", "id": "msg_scripted", "role": "assistant", "status": "completed",
        "content": [{ "type": "output_text", "text": "ok", "annotations": [] }]
    });
    let response = |status: &str, output: Value| {
        json!({
            "id": "resp_scripted", "object": "response", "created_at": 1_700_000_000, "model": model,
            "status": status, "output": output,
            "usage": {
                "input_tokens": 10, "output_tokens": 2, "total_tokens": 12,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            }
        })
    };
    if !stream {
        return (
            "application/json",
            response("completed", json!([message])).to_string(),
        );
    }
    let events = sse(&[
        json!({ "type": "response.created", "response": response("in_progress", json!([])) }),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "message", "id": "msg_scripted", "role": "assistant", "status": "in_progress", "content": [] } }),
        json!({ "type": "response.output_text.delta", "output_index": 0, "content_index": 0, "item_id": "msg_scripted", "delta": "ok" }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": message }),
        json!({ "type": "response.completed", "response": response("completed", json!([message])) }),
    ]);
    ("text/event-stream", events)
}

// ---- the broker and one job ----------------------------------------------------------------

struct Job {
    _broker: Broker,
    dir: PathBuf,
    /// What the sandbox is given: a relay to the broker's socket that records the first line of
    /// every request crossing the channel, model call or proxy request alike.
    socket: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
}

/// Listens at `at` and relays each connection to `broker`, recording its request line.
fn recording_relay(at: &Path, broker: PathBuf) -> Res<Arc<Mutex<Vec<String>>>> {
    let listener = UnixListener::bind(at)?;
    std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o666))?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let (seen, broker) = (Arc::clone(&seen), broker.clone());
            std::thread::spawn(move || {
                let _ = relay_one(client, &broker, &seen);
            });
        }
    });
    Ok(requests)
}

fn relay_one(client: UnixStream, broker: &Path, seen: &Mutex<Vec<String>>) -> Res {
    let mut reader = BufReader::new(client.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if let Ok(mut seen) = seen.lock() {
        seen.push(line.trim_end().to_string());
    }
    let upstream = UnixStream::connect(broker)?;
    (&upstream).write_all(line.as_bytes())?;
    (&upstream).write_all(reader.buffer())?;
    let (mut from_client, mut to_broker) = (client.try_clone()?, upstream.try_clone()?);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut from_client, &mut to_broker);
        let _ = to_broker.shutdown(std::net::Shutdown::Write);
    });
    let (mut from_broker, mut to_client) = (upstream, client);
    let _ = std::io::copy(&mut from_broker, &mut to_client);
    let _ = to_client.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
    Ok(())
}

/// The name in a `hyalos-dns-log: asked <name>` report, found anywhere in the line, so output the
/// harness left without a newline cannot hide one.
fn asked_name(line: &str) -> Option<&str> {
    line.find("hyalos-dns-log: asked ")
        .map(|at| line[at + "hyalos-dns-log: asked ".len()..].trim())
}

/// Every request on the channel that was not a model call: a proxy request, or another path. Only
/// a connection's first request is seen, which is all there is: the broker answers one call per
/// connection and closes it (ADR-0038).
fn not_model_calls(job: &Job) -> Vec<String> {
    job.requests
        .lock()
        .map(|r| {
            r.iter()
                .filter(|l| {
                    !(l.starts_with("POST /v1/messages") || l.starts_with("POST /v1/responses"))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn job(port: u16, seq: u64) -> Res<Job> {
    let providers = Providers::parse(&format!(
        "anthropic.key = sk-image-test\nanthropic.base = http://127.0.0.1:{port}\n\
         openai.key = sk-image-test\nopenai.base = http://127.0.0.1:{port}\n"
    ))?;
    let key = ManifestKey::from_seed([5; 32]);
    let mut broker = Broker::with_sandboxes(
        key.public(),
        Sandboxes {
            models: Arc::new(Models::from_providers(&providers, Arc::new(ThreadSleep))),
            clock: Arc::new(Wall),
        },
    );
    let dir = std::env::temp_dir().join(format!("hyimg-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o711))?;
    let broker_socket = dir.join("broker.sock");
    let socket = dir.join("recorded.sock");
    let deadline = Wall.now().as_millis() + 30 * 60 * 1000;
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(seq),
        role: Role::Actor,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 10_000_000 },
        deadline: Timestamp::from_millis(deadline),
        approval: None,
    };
    let answer = broker.handle(
        ToBroker::StartJob {
            job: manifest.job,
            role: manifest.role,
            manifest: key.sign(&manifest)?.to_bytes()?,
            socket: broker_socket.to_string_lossy().into_owned(),
        },
        &Wall,
    )?;
    assert_eq!(answer, Some(vec![ToEngine::Admitted { job: manifest.job }]));
    let requests = recording_relay(&socket, broker_socket)?;
    Ok(Job {
        _broker: broker,
        dir,
        socket,
        requests,
    })
}

/// Runs `command` in the image as a substrate would, under the DNS log, and returns its exit code
/// and output.
fn sandboxed(socket: &Path, resolv: &Path, command: &[&str]) -> Res<(i32, String)> {
    let out = Command::new("docker")
        .args(["run", "--rm", "--init", "--network", "none", "--read-only"])
        // The container substrate's flags (core/substrate/src/container.rs), so what passes here
        // fits in a job: the same 64 MiB /tmp, which holds $HOME and $CODEX_HOME.
        .args(["--tmpfs", "/tmp:rw,noexec,nosuid,size=64m"])
        .args(["--user", "65534:65534", "--cap-drop", "ALL"])
        .args(["--security-opt", "no-new-privileges"])
        .args(["--memory", "2147483648", "--memory-swap", "2147483648"])
        .args([
            "--pids-limit",
            "512",
            "--pull",
            "never",
            "--log-driver",
            "none",
        ])
        .arg("--mount")
        .arg(format!(
            "type=bind,src={},dst=/run/broker.sock",
            socket.display()
        ))
        .arg("--mount")
        .arg(format!(
            "type=bind,src={},dst=/etc/resolv.conf,readonly",
            resolv.display()
        ))
        .args([
            "--entrypoint",
            "hyalos-dns-log",
            IMAGE,
            "timeout",
            "120",
            "hyalos-forward",
        ])
        .args(command)
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.code().unwrap_or(-1), text))
}

#[test]
fn every_harness_in_the_image_makes_no_lookup_and_reaches_its_model_through_the_broker() -> Res {
    if !image_present()? {
        return Ok(());
    }
    let (port, calls) = scripted_model()?;
    let resolv = std::env::temp_dir().join(format!("hyimg-{}-resolv.conf", std::process::id()));
    std::fs::write(&resolv, "nameserver 127.0.0.1\n")?;

    let harnesses: [(&str, &[&str], &str); 4] = [
        (
            "Claude Code",
            &["claude", "-p", "say ok", "--tools", "Bash"],
            "/v1/messages",
        ),
        (
            "Codex",
            &["codex", "exec", "--skip-git-repo-check", "say ok"],
            "/v1/responses",
        ),
        (
            "Claude Agent SDK",
            &["node", "/opt/hyalos/harness/test/claude-sdk.mjs"],
            "/v1/messages",
        ),
        (
            "OpenAI Agents SDK",
            &["node", "/opt/hyalos/harness/test/openai-sdk.mjs"],
            "/v1/responses",
        ),
    ];
    let mut failures = Vec::new();
    for (seq, (name, command, path)) in (1u64..).zip(harnesses) {
        let job = job(port, seq)?;
        if let Ok(mut c) = calls.lock() {
            c.clear();
        }
        let (code, output) = sandboxed(&job.socket, &resolv, command)?;
        let asked: Vec<&str> = output.lines().filter_map(asked_name).collect();
        let reached = calls
            .lock()
            .map(|c| c.iter().any(|p| p.starts_with(path)))
            .unwrap_or(false);
        let ran = output.contains("hyalos-dns-log: done");
        let other = not_model_calls(&job);
        let said = format!(
            "{name}: exit {code}, lookups {asked:?}, other requests {other:?}, model reached {reached}"
        );
        eprintln!("{said}");
        if code != 0 || !asked.is_empty() || !other.is_empty() || !reached || !ran {
            failures.push(format!("{said}\n{output}"));
        }
    }

    // The controls: with one of the image's settings undone, the tracking call it prevents (Codex's
    // analytics host, spike #72) is seen, so the empty results above mean none was made. Through
    // the proxy variables it arrives on the channel as a request to that host; without them it is a
    // lookup the DNS log sees.
    let analytics = [
        "codex",
        "exec",
        "--skip-git-repo-check",
        "-c",
        "analytics.enabled=true",
        "say ok",
    ];
    let job_a = job(port, 9)?;
    let (_, output) = sandboxed(&job_a.socket, &resolv, &analytics)?;
    let control = not_model_calls(&job_a);
    eprintln!("Codex with analytics on: other requests {control:?}");
    if !control.iter().any(|r| r.contains("chatgpt.com")) {
        failures.push(format!(
            "the channel control's tracking call was not seen\n{output}"
        ));
    }
    let job_b = job(port, 10)?;
    let unproxied: Vec<&str> = ["env", "-u", "HTTPS_PROXY", "-u", "https_proxy"]
        .into_iter()
        .chain(["-u", "HTTP_PROXY", "-u", "http_proxy"])
        .chain(analytics)
        .collect();
    let (_, output) = sandboxed(&job_b.socket, &resolv, &unproxied)?;
    let asked: Vec<&str> = output.lines().filter_map(asked_name).collect();
    eprintln!("Codex with analytics on and no proxy: lookups {asked:?}");
    if !asked.iter().any(|n| n.contains("chatgpt.com")) {
        failures.push(format!("the DNS control saw no lookup\n{output}"));
    }
    // And for Claude Code: with its non-essential traffic allowed again, the call that setting
    // prevents arrives on the channel as a request to Anthropic's host. The other three Claude
    // settings stay set, so this shows this one setting takes effect.
    let job_c = job(port, 11)?;
    let (_, output) = sandboxed(
        &job_c.socket,
        &resolv,
        &[
            "env",
            "-u",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
            "claude",
            "-p",
            "say ok",
            "--tools",
            "Bash",
        ],
    )?;
    let control = not_model_calls(&job_c);
    eprintln!("Claude Code with non-essential traffic on: other requests {control:?}");
    if !control
        .iter()
        .any(|r| r.starts_with("CONNECT api.anthropic.com:"))
    {
        failures.push(format!(
            "the Claude Code control's call was not seen\n{output}"
        ));
    }
    let _ = std::fs::remove_file(&resolv);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}
