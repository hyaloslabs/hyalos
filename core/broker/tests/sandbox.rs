//! Each admitted job's sandbox reaches models through a socket of its own (S3.2, ADR-0038): calls
//! go through the relay under the job's manifest, the key is the broker's, a call past the deadline
//! is a 403, a spent budget a 402, and the socket and the job's meter go when the job ends. A call to a
//! sink the manifest declares no data flow to is refused and reported (ADR-0055).

use std::collections::{BTreeSet, VecDeque};
use std::io::{BufReader, Cursor, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_broker::Broker;
use hyalos_broker::egress::inputs_digest;
use hyalos_broker::model::{
    Anthropic, ApiKey, Meter, OpenAi, Relay, Request, Response, Sleep, Transport, TransportError,
    Upstream, UpstreamResponse,
};
use hyalos_broker::sandbox::{
    Models, ProviderError, Providers, Sandboxes, read_request, write_response,
};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Answer, Budget, Clock, Gate, JobId, MANIFEST_VERSION, Manifest, ManifestKey, ReasonCode, Role,
    Timestamp, flow,
};
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const KEY: &str = "sk-broker-held-key-42";

/// A clock the broker's threads can share.
struct Shared(AtomicU64);

impl Clock for Shared {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

struct NoWait;

impl Sleep for NoWait {
    fn sleep(&self, _: Duration) {}
}

/// A provider that answers every call with usage and records what it was sent.
#[derive(Default)]
struct Provider {
    sent: Mutex<VecDeque<Upstream>>,
}

impl Transport for Provider {
    fn send(&self, request: &Upstream) -> Result<UpstreamResponse, TransportError> {
        if let Ok(mut sent) = self.sent.lock() {
            sent.push_back(Upstream {
                target: request.target.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
            });
        }
        Ok(UpstreamResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: Box::new(Cursor::new(
                br#"{"usage":{"input_tokens":30,"output_tokens":20}}"#.to_vec(),
            )),
        })
    }
}

fn key() -> ManifestKey {
    ManifestKey::from_seed([9; 32])
}

fn manifest(job: u64, role: Role, tokens: u64, deadline: u64) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: hyalos_types::flow::ALL.map(String::from).into(),
        budget: Budget { tokens },
        deadline: Timestamp::from_millis(deadline),
        approval: None,
    }
}

/// A short directory of its own, as the engine makes one per job.
fn job_dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hy-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

struct Rig {
    broker: Broker,
    provider: Arc<Provider>,
    clock: Arc<Shared>,
    meter: Arc<Meter>,
}

fn rig(openai: bool) -> Res<Rig> {
    let provider = Arc::new(Provider::default());
    let meter = Arc::new(Meter::default());
    let relay = |adapter| {
        Relay::new(
            adapter,
            provider.clone(),
            ApiKey::new(KEY).ok_or("key")?,
            meter.clone(),
            Arc::new(NoWait),
        )
        .pipe(Ok::<_, Box<dyn std::error::Error>>)
    };
    let models = Models::new(
        Ok(relay(Arc::new(Anthropic))?),
        if openai {
            Ok(relay(Arc::new(OpenAi))?)
        } else {
            Err("model.no_provider")
        },
        meter.clone(),
    );
    let clock = Arc::new(Shared(AtomicU64::new(100)));
    let broker = Broker::with_sandboxes(
        key().public(),
        Sandboxes {
            models: Arc::new(models),
            egress: hyalos_broker::egress::Egress::public(),
            git: hyalos_broker::git::Git::github(),
            forge: hyalos_broker::forge::Forge::github(),
            clock: clock.clone(),
        },
    );
    Ok(Rig {
        broker,
        provider,
        clock,
        meter,
    })
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

fn start(r: &mut Rig, m: &Manifest, socket: &Path) -> Res<Vec<ToEngine>> {
    let message = ToBroker::StartJob {
        job: m.job,
        role: m.role,
        manifest: key().sign(m)?.to_bytes()?,
        socket: socket.to_string_lossy().into_owned(),
        forge_token: None,
    };
    Ok(r.broker
        .handle(message, r.clock.as_ref())?
        .ok_or("stopped")?)
}

/// Makes one HTTP call over the socket, as a harness's forwarder would, and returns the status and
/// body.
fn call(socket: &Path, path: &str, body: &Value, headers: &[(&str, &str)]) -> Res<(u16, Value)> {
    let mut stream = UnixStream::connect(socket)?;
    let body = serde_json::to_vec(body)?;
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nhost: localhost\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (n, v) in headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    let (head, body) = answer.split_once("\r\n\r\n").ok_or("no head")?;
    let status = head.split(' ').nth(1).ok_or("no status")?.parse::<u16>()?;
    assert!(head.contains("connection: close"), "{head}");
    Ok((status, serde_json::from_str(body).unwrap_or(Value::Null)))
}

fn messages() -> Value {
    json!({ "model": "m", "max_tokens": 10, "messages": [] })
}

#[test]
fn a_job_calls_a_model_through_its_own_socket_with_the_brokers_key() -> Res {
    let mut r = rig(true)?;
    let dir = job_dir("a")?;
    let socket = dir.join("broker.sock");
    let m = manifest(1, Role::Actor, 1_000, 10_000);
    assert_eq!(
        start(&mut r, &m, &socket)?,
        vec![ToEngine::Admitted { job: m.job }]
    );
    assert_eq!(
        std::fs::metadata(&socket)?.permissions().mode() & 0o777,
        0o666,
        "the sandbox's user can connect"
    );

    let (status, _) = call(
        &socket,
        "/v1/messages?beta=true",
        &messages(),
        &[("x-api-key", "placeholder")],
    )?;
    assert_eq!(status, 200);
    let (status, _) = call(
        &socket,
        "/v1/responses",
        &json!({ "model": "m", "input": "hi" }),
        &[("authorization", "Bearer placeholder")],
    )?;
    assert_eq!(status, 200);

    let sent: Vec<_> = r
        .provider
        .sent
        .lock()
        .map_err(|_| "lock")?
        .drain(..)
        .collect();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].headers.contains(&("x-api-key".into(), KEY.into())));
    assert!(
        sent[1]
            .headers
            .contains(&("authorization".into(), format!("Bearer {KEY}")))
    );
    // The meter settles once the response has been written out.
    assert_eq!(r.meter.spent(m.job), 100);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_manifest_is_enforced_on_every_call() -> Res {
    let mut r = rig(true)?;
    let dir = job_dir("b")?;
    let socket = dir.join("broker.sock");
    let reader = manifest(2, Role::Reader, 60, 10_000);
    start(&mut r, &reader, &socket)?;

    let mut with_tools = messages();
    with_tools["tools"] = json!([{ "name": "Bash", "input_schema": {} }]);
    let (status, body) = call(&socket, "/v1/messages", &with_tools, &[])?;
    assert_eq!(status, 403, "a reader's tools are a manifest refusal");
    assert_eq!(body["error"]["type"], "permission_error");

    // 50 is spent after the first call and 10 is left, which the second may still ask for.
    assert_eq!(call(&socket, "/v1/messages", &messages(), &[])?.0, 200);
    assert_eq!(call(&socket, "/v1/messages", &messages(), &[])?.0, 200);
    let (status, body) = call(&socket, "/v1/messages", &messages(), &[])?;
    assert_eq!(status, 402, "the budget of 60 is spent after 50 + 50");
    let message = body["error"]["message"].as_str().ok_or("message")?;
    assert!(message.contains("budget of 60 tokens"), "{message}");

    r.clock.0.store(10_001, Ordering::SeqCst);
    let (status, body) = call(&socket, "/v1/responses", &json!({ "input": "x" }), &[])?;
    assert_eq!(status, 403, "past the deadline");
    assert!(
        body["error"]["message"]
            .as_str()
            .ok_or("message")?
            .contains("deadline")
    );

    let (status, _) = call(&socket, "/v1/models", &json!({}), &[])?;
    assert_eq!(status, 404);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_protocol_with_no_key_is_refused_not_retried() -> Res {
    let mut r = rig(false)?;
    let dir = job_dir("c")?;
    let socket = dir.join("broker.sock");
    start(&mut r, &manifest(3, Role::Actor, 1_000, 10_000), &socket)?;
    let (status, body) = call(&socket, "/v1/responses", &json!({ "input": "x" }), &[])?;
    assert_eq!(status, 403);
    assert!(
        body["error"]["message"]
            .as_str()
            .ok_or("message")?
            .contains("model.no_provider")
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn ending_the_job_closes_its_socket_and_drops_its_meter() -> Res {
    let mut r = rig(true)?;
    let dir = job_dir("d")?;
    let socket = dir.join("broker.sock");
    let m = manifest(4, Role::Actor, 1_000, 10_000);
    start(&mut r, &m, &socket)?;
    call(&socket, "/v1/messages", &messages(), &[])?;
    assert_eq!(r.meter.spent(m.job), 50);

    r.broker
        .handle(ToBroker::EndJob { job: m.job }, r.clock.as_ref())?;
    assert!(!socket.exists(), "the socket file is gone");
    assert!(UnixStream::connect(&socket).is_err());
    assert_eq!(r.meter.spent(m.job), 0);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_call_still_open_when_the_job_ends_never_reaches_the_provider() -> Res {
    let mut r = rig(true)?;
    let dir = job_dir("f")?;
    let socket = dir.join("broker.sock");
    let m = manifest(9, Role::Actor, 1_000, 10_000);
    start(&mut r, &m, &socket)?;
    // Connected, and accepted, but the request not yet sent.
    let mut open = UnixStream::connect(&socket)?;
    std::thread::sleep(Duration::from_millis(200));
    r.broker
        .handle(ToBroker::EndJob { job: m.job }, r.clock.as_ref())?;

    let body = serde_json::to_vec(&messages())?;
    write!(
        open,
        "POST /v1/messages HTTP/1.1\r\ncontent-length: {}\r\n\r\n",
        body.len()
    )?;
    open.write_all(&body)?;
    let mut answer = String::new();
    open.read_to_string(&mut answer)?;
    assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");
    assert!(answer.contains("the job has ended"), "{answer}");
    assert!(r.provider.sent.lock().map_err(|_| "lock")?.is_empty());
    assert_eq!(r.meter.spent(m.job), 0);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_socket_that_cannot_be_made_fails_the_job() -> Res {
    let mut r = rig(true)?;
    let dir = job_dir("e")?;
    let taken = dir.join("broker.sock");
    std::fs::write(&taken, b"")?;
    let refused = |job| ToEngine::Refused {
        job: JobId::from_seq(job),
        reason: ReasonCode::new("broker.socket_unavailable")
            .unwrap_or_else(ReasonCode::unavailable),
    };
    assert_eq!(
        start(&mut r, &manifest(5, Role::Actor, 1, 10_000), &taken)?,
        vec![refused(5)],
        "a path that exists is never taken over"
    );
    assert_eq!(
        start(
            &mut r,
            &manifest(6, Role::Actor, 1, 10_000),
            Path::new("relative.sock")
        )?,
        vec![refused(6)]
    );
    let long = dir.join("x".repeat(120));
    assert_eq!(
        start(&mut r, &manifest(7, Role::Actor, 1, 10_000), &long)?,
        vec![refused(7)]
    );
    // A refused job is not live, so it can be started again once the path is right.
    let socket = dir.join("ok.sock");
    assert_eq!(
        start(&mut r, &manifest(5, Role::Actor, 1, 10_000), &socket)?,
        vec![ToEngine::Admitted {
            job: JobId::from_seq(5)
        }]
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_request_the_broker_cannot_read_is_a_400_family_answer() -> Res {
    for (raw, status) in [
        (&b"GET\r\n\r\n"[..], 400),
        (
            b"POST /v1/messages HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n",
            411,
        ),
        (
            b"POST /v1/messages HTTP/1.1\r\ncontent-length: 99999999999\r\n\r\n",
            413,
        ),
        (
            b"POST /v1/messages HTTP/1.1\r\ncontent-length: 10\r\n\r\nshort",
            400,
        ),
        (b"POST v1/messages HTTP/1.1\r\n\r\n", 400),
    ] {
        assert_eq!(
            read_request(&mut BufReader::new(raw)).err().map(|e| e.0),
            Some(status),
            "{}",
            String::from_utf8_lossy(raw)
        );
    }
    let huge = format!("POST / HTTP/1.1\r\nx: {}\r\n\r\n", "a".repeat(20_000));
    assert_eq!(
        read_request(&mut BufReader::new(huge.as_bytes()))
            .err()
            .map(|e| e.0),
        Some(431)
    );
    let ok: Request = read_request(&mut BufReader::new(
        &b"POST /v1/messages?beta=true HTTP/1.1\r\nX-Api-Key: p\r\ncontent-length: 2\r\n\r\n{}"[..],
    ))
    .map_err(|e| e.1)?;
    assert_eq!(ok.target, "/v1/messages?beta=true");
    assert_eq!(
        ok.headers,
        vec![
            ("x-api-key".to_string(), "p".to_string()),
            ("content-length".to_string(), "2".to_string())
        ]
    );
    assert_eq!(ok.body, b"{}");

    let mut out = Vec::new();
    write_response(
        &mut out,
        Response {
            status: 402,
            content_type: "application/json\r\nx-evil: 1".into(),
            body: Box::new(Cursor::new(b"{}".to_vec())),
            reason: None,
        },
    )?;
    let text = String::from_utf8(out)?;
    assert!(text.starts_with("HTTP/1.1 402 "));
    assert!(!text.contains("x-evil"), "{text}");
    Ok(())
}

#[test]
fn the_key_file_names_providers_and_never_echoes_a_value() -> Res {
    let p = Providers::parse(
        "# keys\n\nanthropic.key = sk-ant-1\nopenai.key=sk-2\nopenai.base = http://127.0.0.1:11434\n",
    )?;
    let anthropic = p.anthropic.ok_or("anthropic")?;
    assert_eq!(anthropic.base, "https://api.anthropic.com");
    assert_eq!(p.openai.ok_or("openai")?.base, "http://127.0.0.1:11434");
    assert!(!format!("{anthropic:?}").contains("sk-ant-1"));

    for (text, error) in [
        ("anthropic.key sk-1", ProviderError::Line(1)),
        ("\nfoo.key = sk-1", ProviderError::Line(2)),
        ("openai.key =", ProviderError::Line(1)),
        ("openai.key = a\nopenai.key = b", ProviderError::Repeated(2)),
        ("openai.key = has space", ProviderError::Key("openai")),
        (
            "anthropic.base = http://127.0.0.1:1",
            ProviderError::BaseWithoutKey("anthropic"),
        ),
    ] {
        let got = Providers::parse(text).err();
        assert_eq!(got.as_ref(), Some(&error), "{text}");
        let said = got.map(|e| e.to_string()).unwrap_or_default();
        assert!(
            !said.contains("sk-") && !said.contains("has space"),
            "{said}"
        );
    }
    assert!(Providers::parse("")?.anthropic.is_none());

    // Nothing in the file can turn certificate checks off: there is no such setting (ADR-0039).
    for setting in [
        "openai.insecure = true",
        "anthropic.verify = false",
        "openai.ca = /tmp/ca",
    ] {
        assert_eq!(
            Providers::parse(&format!("openai.key = sk-1\n{setting}")).err(),
            Some(ProviderError::Line(2)),
            "{setting}"
        );
    }

    // A base the transport refuses (plain HTTP off this host) makes calls 403 with its code.
    let models = Models::from_providers(
        &Providers::parse("anthropic.key = sk-1\nanthropic.base = http://10.0.0.5:8000\n")?,
        Arc::new(NoWait),
    );
    let mut r = rig(true)?;
    let m = manifest(8, Role::Actor, 10, 10_000);
    let request = Request {
        method: "POST".into(),
        target: "/v1/messages".into(),
        headers: Vec::new(),
        body: serde_json::to_vec(&messages())?,
    };
    let response = models.answer(&m, request, r.clock.as_ref());
    assert_eq!(response.status, 403);
    assert_eq!(
        response.reason.map(|c| c.as_str().to_string()),
        Some("transport.not_loopback".into())
    );
    r.broker.handle(ToBroker::Shutdown, r.clock.as_ref())?;
    Ok(())
}

/// Sends `request` as it is and returns the whole answer.
fn raw(socket: &Path, request: &str) -> Res<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(request.as_bytes())?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    Ok(answer)
}

#[test]
fn data_goes_only_where_the_manifest_declares_a_flow() -> Res {
    let mut r = rig(true)?;
    let (to, reports) = std::sync::mpsc::channel();
    r.broker = r.broker.report_to(to);
    let denials = || {
        std::thread::sleep(Duration::from_millis(50));
        reports
            .try_iter()
            .filter_map(|m| match m {
                ToEngine::Gate {
                    gate: Gate::Manifest,
                    answer: Answer::Deny,
                    reason,
                    inputs,
                    ..
                } => Some((reason.as_str().to_string(), inputs)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let refused = |sink: &str| ("flow.not_declared".to_string(), inputs_digest(sink));

    // No flow to a model: a model call is refused in the provider's shape and never sent.
    let dir = job_dir("flows-a")?;
    let socket = dir.join("broker.sock");
    let mut m = manifest(20, Role::Actor, 1_000, 10_000);
    m.data_flows.remove(flow::MODEL);
    start(&mut r, &m, &socket)?;
    let (status, body) = call(&socket, "/v1/messages", &messages(), &[])?;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["type"], "permission_error");
    let (status, body) = call(&socket, "/v1/responses", &json!({ "input": "x" }), &[])?;
    assert_eq!(status, 403);
    assert!(
        body["error"]["message"]
            .as_str()
            .ok_or("message")?
            .contains("flow.not_declared"),
        "{body}"
    );
    assert!(r.provider.sent.lock().map_err(|_| "lock")?.is_empty());
    // Reported once for the flow, however many calls it refused.
    assert_eq!(denials(), vec![refused(flow::MODEL)]);
    let _ = std::fs::remove_dir_all(&dir);

    // Only a flow to a model: a push, a pull request and a web request are refused; a fetch carries
    // nothing of the job's and goes on to the git proxy, which wants a token.
    let dir = job_dir("flows-b")?;
    let socket = dir.join("broker.sock");
    let mut m = manifest(21, Role::Actor, 1_000, 10_000);
    m.data_flows = [flow::MODEL.to_string()].into();
    m.actions = ["read".to_string()].into();
    start(&mut r, &m, &socket)?;
    for request in [
        "GET /git/hyaloslabs/hyalos.git/info/refs?service=git-receive-pack HTTP/1.1\r\nhost: x\r\n\r\n",
        "POST /git/hyaloslabs/hyalos.git/git-receive-pack HTTP/1.1\r\nhost: x\r\ncontent-length: 0\r\n\r\n",
        "POST /forge/repos/hyaloslabs/hyalos/pulls HTTP/1.1\r\nhost: x\r\ncontent-length: 0\r\n\r\n",
        "CONNECT example.com:443 HTTP/1.1\r\nhost: example.com:443\r\n\r\n",
        "GET http://example.com/ HTTP/1.1\r\nhost: example.com\r\n\r\n",
    ] {
        let answer = raw(&socket, request)?;
        assert!(answer.starts_with("HTTP/1.1 403"), "{request}: {answer}");
        assert!(answer.contains("flow.not_declared"), "{request}: {answer}");
    }
    assert_eq!(denials(), vec![refused(flow::FORGE), refused(flow::WEB)]);
    let fetch = raw(
        &socket,
        "GET /git/hyaloslabs/hyalos.git/info/refs?service=git-upload-pack HTTP/1.1\r\nhost: x\r\n\r\n",
    )?;
    assert!(fetch.contains("git.no_token"), "{fetch}");
    assert_eq!(call(&socket, "/v1/messages", &messages(), &[])?.0, 200);
    assert_eq!(denials(), vec![]);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_readers_call_with_tools_is_refused_and_reported_by_the_role_gate() -> Res {
    let mut r = rig(true)?;
    let (to, reports) = std::sync::mpsc::channel();
    r.broker = r.broker.report_to(to);
    let dir = job_dir("reader-tools")?;
    let socket = dir.join("broker.sock");
    start(&mut r, &manifest(30, Role::Reader, 1_000, 10_000), &socket)?;
    let mut with_tools = messages();
    with_tools["tools"] = json!([{ "name": "bash", "input_schema": { "type": "object" } }]);
    for _ in 0..2 {
        let (status, body) = call(&socket, "/v1/messages", &with_tools, &[])?;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["type"], "permission_error");
    }
    let (status, _) = call(
        &socket,
        "/v1/responses",
        &json!({ "input": "x", "tools": [{ "type": "function", "name": "f" }] }),
        &[],
    )?;
    assert_eq!(status, 403);
    assert!(r.provider.sent.lock().map_err(|_| "lock")?.is_empty());
    // A reader with no tools still reaches the provider, and is not reported.
    let (status, _) = call(&socket, "/v1/messages", &messages(), &[])?;
    assert_eq!(status, 200);
    std::thread::sleep(Duration::from_millis(50));
    let reported: Vec<_> = reports
        .try_iter()
        .map(|m| match m {
            ToEngine::Gate {
                job,
                gate,
                answer,
                reason,
                inputs,
            } => (job.seq(), gate, answer, reason.as_str().to_string(), inputs),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    // Once for each path, however many calls it refused.
    let deny = |path: &str| {
        (
            30,
            Gate::Role,
            Answer::Deny,
            "model.reader_tools".to_string(),
            inputs_digest(path),
        )
    };
    assert_eq!(reported, vec![deny("/v1/messages"), deny("/v1/responses")]);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
