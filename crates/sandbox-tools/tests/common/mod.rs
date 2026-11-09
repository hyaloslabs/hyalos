//! What the sandbox image tests share: a scripted model, a real broker serving one job, a record of
//! every request on the job's channel, and a way to run a command in the image as the container
//! substrate would (ADR-0040, ADR-0042).

#![allow(dead_code, reason = "each test file uses part of this")]

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
use hyalos_broker::egress::Egress;
use hyalos_broker::model::ThreadSleep;
use hyalos_broker::sandbox::{Models, Providers, Sandboxes};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Budget, Clock, JobId, MANIFEST_VERSION, Manifest, ManifestKey, Role, Timestamp,
};
use serde_json::{Value, json};

pub type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub const IMAGE: &str = "hyalos-sandbox:test";

pub struct Wall;

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
pub fn image_present() -> Res<bool> {
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

/// A model on loopback that answers every call with a short text, or, when told to, first asks the
/// harness to fetch a URL or run a command with its own tool. It records the paths and bodies it was
/// sent.
#[derive(Clone)]
pub struct Model {
    pub port: u16,
    pub calls: Arc<Mutex<Vec<String>>>,
    pub bodies: Arc<Mutex<Vec<Value>>>,
    /// The URL the next run's harness is asked to fetch, if any.
    pub fetch: Arc<Mutex<Option<String>>>,
    /// The shell command the next run's harness is asked to run, if any: Claude Code with its Bash
    /// tool, Codex with its shell.
    pub run: Arc<Mutex<Option<String>>>,
}

impl Model {
    pub fn clear(&self) {
        if let Ok(mut c) = self.calls.lock() {
            c.clear();
        }
        if let Ok(mut b) = self.bodies.lock() {
            b.clear();
        }
    }

    pub fn reached(&self, path: &str) -> bool {
        self.calls
            .lock()
            .map(|c| c.iter().any(|p| p.starts_with(path)))
            .unwrap_or(false)
    }
}

pub fn scripted_model() -> Res<Model> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let model = Model {
        port: listener.local_addr()?.port(),
        calls: Arc::default(),
        bodies: Arc::default(),
        fetch: Arc::default(),
        run: Arc::default(),
    };
    let shared = model.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let model = shared.clone();
            std::thread::spawn(move || {
                let _ = answer(stream, &model);
            });
        }
    });
    Ok(model)
}

fn answer(stream: TcpStream, model: &Model) -> Res {
    let model_state = model;
    let seen = &model.calls;
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
    if let Ok(mut bodies) = model.bodies.lock() {
        bodies.push(body.clone());
    }
    let stream_wanted = body["stream"].as_bool().unwrap_or(false);
    let model = body["model"].as_str().unwrap_or("scripted").to_string();
    let (content_type, reply) = if path.contains("count_tokens") {
        (
            "application/json",
            json!({ "input_tokens": 10 }).to_string(),
        )
    } else if path.starts_with("/v1/messages") {
        if let Some(command) = wanted(&model_state.run, &body, "Bash") {
            let input = json!({ "command": command, "description": "Report the result" });
            anthropic_tool(&model, stream_wanted, "Bash", &input)
        } else if let Some(url) = wanted(&model_state.fetch, &body, "WebFetch") {
            let input = json!({ "url": url, "prompt": "Say what the page says." });
            anthropic_tool(&model, stream_wanted, "WebFetch", &input)
        } else {
            anthropic(&model, stream_wanted)
        }
    } else if let Some(command) = wanted(&model_state.run, &body, "exec_command") {
        responses_exec(&model, stream_wanted, &command)
    } else if let Some(url) = wanted(&model_state.fetch, &body, "exec_command") {
        responses_exec(&model, stream_wanted, &curl(&url))
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

/// What to ask for with `tool` (a URL or a command), when the model is told to, the call offers that
/// tool, and no tool has answered yet in this conversation.
fn wanted(told: &Mutex<Option<String>>, body: &Value, tool: &str) -> Option<String> {
    let url = told.lock().ok()?.clone()?;
    let offered = body["tools"]
        .as_array()?
        .iter()
        .any(|t| t["name"].as_str() == Some(tool));
    let text = body.to_string();
    let answered = text.contains("\"tool_result\"") || text.contains("\"function_call_output\"");
    (offered && !answered).then_some(url)
}

/// The command a Codex run is asked to run for `url`.
pub fn curl(url: &str) -> String {
    format!("curl -sS --max-time 20 --cacert /opt/hyalos/harness/test/ca.pem {url}")
}

fn anthropic_tool(model: &str, stream: bool, tool: &str, input: &Value) -> (&'static str, String) {
    let usage = json!({ "input_tokens": 10, "output_tokens": 2 });
    let block = json!({ "type": "tool_use", "id": "toolu_scripted", "name": tool, "input": input });
    if !stream {
        let message = json!({
            "id": "msg_scripted", "type": "message", "role": "assistant", "model": model,
            "content": [block], "stop_reason": "tool_use", "stop_sequence": null, "usage": usage
        });
        return ("application/json", message.to_string());
    }
    let start = json!({
        "id": "msg_scripted", "type": "message", "role": "assistant", "model": model,
        "content": [], "stop_reason": null, "stop_sequence": null, "usage": usage
    });
    let events = sse(&[
        json!({ "type": "message_start", "message": start }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": "toolu_scripted", "name": tool, "input": {} } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": input.to_string() } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use", "stop_sequence": null }, "usage": { "output_tokens": 2 } }),
        json!({ "type": "message_stop" }),
    ]);
    ("text/event-stream", events)
}

fn responses_exec(model: &str, stream: bool, command: &str) -> (&'static str, String) {
    let arguments = json!({ "cmd": command }).to_string();
    let call = |status: &str, arguments: &str| {
        json!({
            "type": "function_call", "id": "fc_scripted", "call_id": "call_scripted",
            "name": "exec_command", "arguments": arguments, "status": status
        })
    };
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
    let done = call("completed", &arguments);
    if !stream {
        return (
            "application/json",
            response("completed", json!([done])).to_string(),
        );
    }
    let events = sse(&[
        json!({ "type": "response.created", "response": response("in_progress", json!([])) }),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": call("in_progress", "") }),
        json!({ "type": "response.function_call_arguments.delta", "output_index": 0, "item_id": "fc_scripted", "delta": arguments }),
        json!({ "type": "response.function_call_arguments.done", "output_index": 0, "item_id": "fc_scripted", "arguments": arguments }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": done }),
        json!({ "type": "response.completed", "response": response("completed", json!([done])) }),
    ]);
    ("text/event-stream", events)
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

pub struct Job {
    /// The broker serving the job, which a test asks for what the job submitted.
    pub broker: Broker,
    dir: PathBuf,
    /// What the sandbox is given: a relay to the broker's socket that records the first line of
    /// every request crossing the channel, model call or proxy request alike.
    pub socket: PathBuf,
    pub requests: Arc<Mutex<Vec<String>>>,
    /// What the broker reported to the engine for this job: the egress gate's refusals.
    pub reports: std::sync::mpsc::Receiver<ToEngine>,
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
pub fn asked_name(line: &str) -> Option<&str> {
    line.find("hyalos-dns-log: asked ")
        .map(|at| line[at + "hyalos-dns-log: asked ".len()..].trim())
}

/// Every request on the channel that was not a model call: a proxy request, or another path. Only
/// a connection's first request is seen, which is all there is: the broker answers one call per
/// connection and closes it (ADR-0038).
pub fn not_model_calls(job: &Job) -> Vec<String> {
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

pub fn job(port: u16, seq: u64) -> Res<Job> {
    job_with(port, seq, &[], Egress::public())
}

/// A job whose manifest allows `hosts`, reached through `egress`.
pub fn job_with(port: u16, seq: u64, hosts: &[&str], egress: Egress) -> Res<Job> {
    job_spec(
        port,
        seq,
        Spec {
            hosts,
            egress,
            ..Spec::default()
        },
    )
}

/// What a test job's manifest and broker are given, beyond the scripted model.
pub struct Spec<'a> {
    pub hosts: &'a [&'a str],
    pub egress: Egress,
    pub git: hyalos_broker::git::Git,
    pub repo: &'a str,
    pub actions: &'a [&'a str],
    pub forge_token: Option<&'a str>,
    /// The result the job must submit (ADR-0064).
    pub result: Option<hyalos_types::workflow::Output>,
}

impl Default for Spec<'_> {
    fn default() -> Self {
        Self {
            hosts: &[],
            egress: Egress::public(),
            git: hyalos_broker::git::Git::github(),
            repo: "hyaloslabs/hyalos",
            actions: &[],
            forge_token: None,
            result: None,
        }
    }
}

/// A job as `spec` says.
pub fn job_spec(port: u16, seq: u64, spec: Spec<'_>) -> Res<Job> {
    let Spec {
        hosts,
        egress,
        git,
        repo,
        actions,
        forge_token,
        result,
    } = spec;
    let providers = Providers::parse(&format!(
        "anthropic.key = sk-image-test\nanthropic.base = http://127.0.0.1:{port}\n\
         openai.key = sk-image-test\nopenai.base = http://127.0.0.1:{port}\n"
    ))?;
    let key = ManifestKey::from_seed([5; 32]);
    let (reports_to, reports) = std::sync::mpsc::channel();
    let mut broker = Broker::with_sandboxes(
        key.public(),
        Sandboxes {
            models: Arc::new(Models::from_providers(&providers, Arc::new(ThreadSleep))),
            egress,
            git,
            forge: hyalos_broker::forge::Forge::github(),
            clock: Arc::new(Wall),
        },
    )
    .report_to(reports_to);
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
        repo: repo.into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: actions.iter().map(|a| a.to_string()).collect(),
        egress_hosts: hosts.iter().map(|h| h.to_string()).collect(),
        tools: BTreeSet::new(),
        data_flows: hyalos_types::flow::ALL.map(String::from).into(),
        budget: Budget { tokens: 10_000_000 },
        deadline: Timestamp::from_millis(deadline),
        approval: None,
        handles: Default::default(),
        result,
        closes: None,
    };
    let answer = broker.handle(
        ToBroker::StartJob {
            job: manifest.job,
            role: manifest.role,
            manifest: key.sign(&manifest)?.to_bytes()?,
            socket: broker_socket.to_string_lossy().into_owned(),
            forge_token: forge_token
                .map(|t| {
                    hyalos_types::ForgeToken::new(t, Timestamp::from_millis(deadline))
                        .ok_or("token")
                })
                .transpose()?,
        },
        &Wall,
    )?;
    assert_eq!(answer, Some(vec![ToEngine::Admitted { job: manifest.job }]));
    let requests = recording_relay(&socket, broker_socket)?;
    Ok(Job {
        broker,
        dir,
        socket,
        requests,
        reports,
    })
}

/// Runs `command` in the image as a substrate would, under the DNS log, and returns its exit code
/// and output.
pub fn sandboxed(socket: &Path, resolv: &Path, command: &[&str]) -> Res<(i32, String)> {
    sandboxed_with(socket, resolv, &[], command)
}

/// As [`sandboxed`], with `env` added to the container's environment.
pub fn sandboxed_with(
    socket: &Path,
    resolv: &Path,
    env: &[(&str, &str)],
    command: &[&str],
) -> Res<(i32, String)> {
    sandboxed_over(socket, resolv, env, &[], command)
}

/// As [`sandboxed_with`], with each `(file, path)` in `over` mounted read-only over that path in the
/// image, to undo one of its settings for a control.
pub fn sandboxed_over(
    socket: &Path,
    resolv: &Path,
    env: &[(&str, &str)],
    over: &[(&Path, &str)],
    command: &[&str],
) -> Res<(i32, String)> {
    let over: Vec<String> = over
        .iter()
        .flat_map(|(file, path)| {
            [
                "--mount".to_string(),
                format!("type=bind,src={},dst={path},readonly", file.display()),
            ]
        })
        .collect();
    let env: Vec<String> = env
        .iter()
        .flat_map(|(k, v)| ["--env".to_string(), format!("{k}={v}")])
        .collect();
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
        .args(&env)
        .args(&over)
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
