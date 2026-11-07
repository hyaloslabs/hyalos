//! Opening a pull request (S3.12, ADR-0048): a job asks its socket for a pull request as it would ask
//! GitHub's REST API, and the broker opens it with the job's token only for the manifest's repository,
//! from a branch inside the prefix, into the manifest's base, with the `open_pr` action, and only once
//! the title and body have passed the content gate. Every refusal reaches GitHub not at all and is
//! reported to the engine as the manifest or content gate's `deny`.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_broker::Broker;
use hyalos_broker::egress::{Egress, inputs_digest};
use hyalos_broker::forge::Forge;
use hyalos_broker::git::Git;
use hyalos_broker::model::{Anthropic, ApiKey, HttpTransport, Meter, Relay, Sleep};
use hyalos_broker::sandbox::{Models, Sandboxes};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Answer, Budget, Clock, Digest, ForgeToken, Gate, JobId, MANIFEST_VERSION, Manifest,
    ManifestKey, Role, Timestamp,
};
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const TOKEN: &str = "ghs_pull_request_test_token";
const PROVIDER_KEY: &str = "sk-ant-provider-key-for-pulls";

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

fn key() -> ManifestKey {
    ManifestKey::from_seed([13; 32])
}

fn manifest(job: u64, actions: &[&str], pr_base: Option<&str>) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Actor,
        tainted: true,
        repo: "octo/repo".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: pr_base.map(str::to_string),
        actions: actions.iter().map(|a| a.to_string()).collect(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: hyalos_types::flow::ALL.map(String::from).into(),
        budget: Budget { tokens: 1_000 },
        deadline: Timestamp::from_millis(10_000),
        approval: None,
        handles: Default::default(),
        result: None,
    }
}

/// One request GitHub's REST API was sent.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    authorization: String,
    body: Value,
}

/// GitHub's REST API on loopback: a pull request is created as number 5, unless it is told to answer
/// otherwise.
struct Api {
    port: u16,
    reply: Arc<Mutex<(u16, String)>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Api {
    fn start() -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let created =
            json!({ "number": 5, "html_url": "https://github.com/octo/repo/pull/5", "id": 1 });
        let reply = Arc::new(Mutex::new((201, created.to_string())));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (r, s) = (Arc::clone(&reply), Arc::clone(&seen));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = serve_one(stream, &r, &s);
            }
        });
        Ok(Self { port, reply, seen })
    }

    fn answer_with(&self, status: u16, body: &str) {
        if let Ok(mut reply) = self.reply.lock() {
            *reply = (status, body.to_string());
        }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn serve_one(
    stream: std::net::TcpStream,
    reply: &Mutex<(u16, String)>,
    seen: &Mutex<Vec<Seen>>,
) -> Res {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or_default().to_string();
    let path = words.next().unwrap_or_default().to_string();
    let (mut length, mut authorization) = (0usize, String::new());
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((n, v)) = h.split_once(':') {
            match n.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse()?,
                "authorization" => authorization = v.trim().to_string(),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let authorized = authorization == format!("Bearer {TOKEN}");
    if let Ok(mut seen) = seen.lock() {
        seen.push(Seen {
            method,
            path,
            authorization,
            body,
        });
    }
    let (status, text) = if authorized {
        reply.lock().map(|r| r.clone()).unwrap_or_default()
    } else {
        (401, r#"{"message":"Bad credentials"}"#.into())
    };
    let mut out = stream;
    out.write_all(
        format!(
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
            text.len()
        )
        .as_bytes(),
    )?;
    Ok(())
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hy-pr-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

struct Job {
    _broker: Broker,
    socket: PathBuf,
    reports: Receiver<ToEngine>,
    clock: Arc<Shared>,
}

fn start(api: &Api, m: &Manifest, token: Option<&str>) -> Res<Job> {
    let clock = Arc::new(Shared(AtomicU64::new(100)));
    let (to, reports) = mpsc::channel();
    let meter = Arc::new(Meter::default());
    // A provider the content gate must keep the key of out of outbound text; it is never called.
    let relay = Relay::new(
        Arc::new(Anthropic),
        Arc::new(HttpTransport::new("http://127.0.0.1:1").map_err(|e| e.0)?),
        ApiKey::new(PROVIDER_KEY).ok_or("key")?,
        meter.clone(),
        Arc::new(NoWait),
    );
    let sandboxes = Sandboxes {
        models: Arc::new(Models::new(Ok(relay), Err("model.no_provider"), meter)),
        egress: Egress::public(),
        git: Git::github(),
        forge: Forge::with_base(&format!("http://127.0.0.1:{}", api.port)),
        clock: clock.clone(),
    };
    let mut broker = Broker::with_sandboxes(key().public(), sandboxes).report_to(to);
    let socket = dir(&format!("sock-{}", m.job.seq()))?.join("job.sock");
    let answers = broker
        .handle(
            ToBroker::StartJob {
                job: m.job,
                role: m.role,
                manifest: key().sign(m)?.to_bytes()?,
                socket: socket.to_string_lossy().into_owned(),
                forge_token: token
                    .map(|t| ForgeToken::new(t, Timestamp::from_millis(99_999)).ok_or("token"))
                    .transpose()?,
            },
            clock.as_ref(),
        )?
        .ok_or("stopped")?;
    assert_eq!(answers, vec![ToEngine::Admitted { job: m.job }]);
    Ok(Job {
        _broker: broker,
        socket,
        reports,
        clock,
    })
}

/// Sends `body` to `path` on the job's socket, as the harness would, and returns the status and body.
fn post(socket: &Path, path: &str, body: &str) -> Res<(u16, Value)> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nhost: api.github.com\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    let status = answer
        .split_whitespace()
        .nth(1)
        .ok_or("no status")?
        .parse()?;
    let text = answer
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or_default();
    Ok((
        status,
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())),
    ))
}

const PULLS: &str = "/forge/repos/octo/repo/pulls";

fn pull(title: &str, body: &str, head: &str, base: &str) -> String {
    json!({ "title": title, "body": body, "head": head, "base": base }).to_string()
}

impl Job {
    fn open(&self, request: &str) -> Res<(u16, Value)> {
        post(&self.socket, PULLS, request)
    }

    /// The denials reported so far, as (gate, reason, inputs).
    fn denials(&self) -> Vec<(Gate, String, Digest)> {
        std::thread::sleep(Duration::from_millis(50));
        self.reports
            .try_iter()
            .filter_map(|m| match m {
                ToEngine::Gate {
                    gate,
                    answer: Answer::Deny,
                    reason,
                    inputs,
                    ..
                } => Some((gate, reason.as_str().to_string(), inputs)),
                _ => None,
            })
            .collect()
    }
}

/// The code a refusal names; the broker writes refusals as plain text.
fn reason(answer: &Value) -> &str {
    answer
        .as_str()
        .and_then(|t| t.trim().strip_prefix("the broker refused this request: "))
        .unwrap_or_default()
}

#[test]
fn a_job_opens_a_pull_request_from_its_branch_into_its_base() -> Res {
    let api = Api::start()?;
    let job = start(&api, &manifest(1, &["open_pr"], Some("main")), Some(TOKEN))?;
    let (status, answer) = job.open(&pull(
        "Fix the parser",
        "Closes #12.\n\n修复 · Исправить\n\t- one",
        "hyalos/fix-12",
        "main",
    ))?;
    assert_eq!(status, 201, "{answer}");
    // The harness gets the number and the page, nothing else GitHub said.
    assert_eq!(
        answer,
        json!({ "number": 5, "html_url": "https://github.com/octo/repo/pull/5" })
    );
    let seen = api.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/repos/octo/repo/pulls")
    );
    assert_eq!(seen[0].authorization, format!("Bearer {TOKEN}"));
    assert_eq!(
        seen[0].body,
        json!({
            "title": "Fix the parser",
            "body": "Closes #12.\n\n修复 · Исправить\n\t- one",
            "head": "hyalos/fix-12",
            "base": "main",
            "draft": false,
            "maintainer_can_modify": false,
        })
    );
    assert!(job.denials().is_empty());
    Ok(())
}

#[test]
fn a_pull_request_outside_the_manifest_is_refused_and_reported() -> Res {
    let api = Api::start()?;
    let job = start(&api, &manifest(2, &["open_pr"], Some("main")), Some(TOKEN))?;
    let cases = [
        (
            "/forge/repos/octo/other/pulls",
            pull("t", "b", "hyalos/x", "main"),
            "forge.repo_not_allowed",
            "octo/other",
        ),
        (
            PULLS,
            pull("t", "b", "hyalos/x", "release"),
            "forge.base_not_allowed",
            "release",
        ),
        (
            PULLS,
            pull("t", "b", "hyalos/x", "main\u{0}"),
            "forge.base_not_allowed",
            "main\u{0}",
        ),
        (
            PULLS,
            pull("t", "b", "main", "main"),
            "forge.head_not_allowed",
            "main",
        ),
        (
            PULLS,
            pull("t", "b", "other/x", "main"),
            "forge.head_not_allowed",
            "other/x",
        ),
        (
            PULLS,
            pull("t", "b", "stranger:hyalos/x", "main"),
            "forge.head_not_allowed",
            "stranger:hyalos/x",
        ),
        (
            PULLS,
            pull("t", "b", "hyalos/../main", "main"),
            "forge.head_not_allowed",
            "hyalos/../main",
        ),
    ];
    for (path, request, code, input) in &cases {
        let (status, answer) = post(&job.socket, path, request)?;
        assert_eq!((status, reason(&answer)), (403, *code), "{path} {request}");
        assert_eq!(
            job.denials(),
            vec![(Gate::Manifest, code.to_string(), inputs_digest(input))],
            "{request}"
        );
    }
    // Case does not make another repository the manifest's.
    let (status, _) = post(
        &job.socket,
        "/forge/repos/Octo/Repo/pulls",
        &pull("t", "b", "hyalos/x", "main"),
    )?;
    assert_eq!(status, 201);
    assert_eq!(api.seen().len(), 1, "only the allowed one reached GitHub");
    Ok(())
}

#[test]
fn a_pull_request_needs_the_action_a_base_and_a_token() -> Res {
    let api = Api::start()?;
    let request = pull("t", "b", "hyalos/x", "main");
    let job = start(&api, &manifest(3, &["push"], Some("main")), Some(TOKEN))?;
    let (status, answer) = job.open(&request)?;
    assert_eq!((status, reason(&answer)), (403, "forge.action_not_allowed"));
    assert_eq!(
        job.denials(),
        vec![(
            Gate::Manifest,
            "forge.action_not_allowed".into(),
            inputs_digest("open_pr")
        )]
    );
    // No base in the manifest: it opens no pull request at all.
    let job = start(&api, &manifest(4, &["open_pr"], None), Some(TOKEN))?;
    let (status, answer) = job.open(&request)?;
    assert_eq!((status, reason(&answer)), (403, "forge.base_not_allowed"));
    let job = start(&api, &manifest(5, &["open_pr"], Some("main")), None)?;
    let (status, answer) = job.open(&request)?;
    assert_eq!((status, reason(&answer)), (403, "forge.no_token"));
    // Past the deadline.
    let job = start(&api, &manifest(6, &["open_pr"], Some("main")), Some(TOKEN))?;
    job.clock.0.store(20_000, Ordering::SeqCst);
    let (status, answer) = job.open(&request)?;
    assert_eq!((status, reason(&answer)), (403, "admission.expired"));
    // Only a create is served: merging, issues and anything else under /forge/ are not. A sandbox
    // never merges; a merge is the engine's `merge` message for an approved, untainted job alone
    // (ADR-0071), and a manifest asking for one without its approval is not even admitted.
    let job = start(&api, &manifest(7, &["open_pr"], Some("main")), Some(TOKEN))?;
    for path in [
        "/forge/repos/octo/repo/pulls/5/merge",
        "/forge/repos/octo/repo/issues",
        "/forge/graphql",
    ] {
        let (status, answer) = post(&job.socket, path, &request)?;
        assert_eq!(
            (status, reason(&answer)),
            (404, "forge.route_refused"),
            "{path}"
        );
    }
    // A field the broker does not pass on, a missing title, a title too long: refused before GitHub.
    for bad in [
        json!({ "title": "t", "head": "hyalos/x", "base": "main", "maintainer_can_modify": true })
            .to_string(),
        json!({ "head": "hyalos/x", "base": "main" }).to_string(),
        pull(" ", "b", "hyalos/x", "main"),
        pull(&"t".repeat(257), "b", "hyalos/x", "main"),
        "not json".to_string(),
    ] {
        let (status, answer) = job.open(&bad)?;
        assert_eq!(reason(&answer), "forge.bad_request", "{bad}");
        assert!(status == 400 || status == 422, "{status} {bad}");
    }
    assert!(api.seen().is_empty(), "{:?}", api.seen());
    Ok(())
}

#[test]
fn the_content_gate_refuses_a_credential_or_hidden_text_and_reports_it() -> Res {
    let api = Api::start()?;
    let job = start(&api, &manifest(8, &["open_pr"], Some("main")), Some(TOKEN))?;
    let cases = [
        ("Fix", format!("token {TOKEN} leaked"), "content.credential"),
        (TOKEN, "b".to_string(), "content.credential"),
        ("Fix", format!("key={PROVIDER_KEY}"), "content.credential"),
        (
            "Fix",
            "looks fine\u{202e}txt.exe".to_string(),
            "content.hidden_text",
        ),
        ("Fix\u{200b}", "b".to_string(), "content.hidden_text"),
        (
            "Fix",
            "a hidden\u{e0049}\u{e0067}\u{e006e} note".to_string(),
            "content.hidden_text",
        ),
        ("Fix", "bell\u{7}".to_string(), "content.hidden_text"),
    ];
    for (title, body, code) in &cases {
        let (status, answer) = job.open(&pull(title, body, "hyalos/x", "main"))?;
        assert_eq!(
            (status, reason(&answer)),
            (403, *code),
            "{title:?} {body:?}"
        );
        assert_eq!(
            job.denials(),
            vec![(
                Gate::Content,
                code.to_string(),
                inputs_digest(&format!("{title}\n{body}"))
            )],
            "{body:?}"
        );
    }
    assert!(api.seen().is_empty(), "nothing gated reached GitHub");
    Ok(())
}

#[test]
fn what_github_answers_is_mapped_and_never_relayed() -> Res {
    let api = Api::start()?;
    let job = start(&api, &manifest(9, &["open_pr"], Some("main")), Some(TOKEN))?;
    let request = pull("t", "b", "hyalos/x", "main");
    for (status, body, want_status, want) in [
        (
            422,
            r#"{"message":"A pull request already exists"}"#,
            422,
            "forge.pull_rejected",
        ),
        (403, "{}", 403, "forge.upstream_refused"),
        (404, "{}", 404, "forge.upstream_not_found"),
        (500, "secret stack trace", 502, "forge.upstream_status"),
        (201, "not json", 502, "forge.malformed"),
        (
            201,
            r#"{"number":5,"html_url":"javascript:alert(1)"}"#,
            502,
            "forge.malformed",
        ),
        (
            201,
            r#"{"number":5,"html_url":"httpx://evil.example"}"#,
            502,
            "forge.malformed",
        ),
        (
            201,
            r#"{"html_url":"https://github.com/octo/repo/pull/5"}"#,
            502,
            "forge.malformed",
        ),
    ] {
        api.answer_with(status, body);
        let (got, answer) = job.open(&request)?;
        assert_eq!(
            (got, reason(&answer)),
            (want_status, want),
            "{status} {body}"
        );
        assert!(
            !answer.to_string().contains("already exists") && !answer.to_string().contains("stack")
        );
    }
    Ok(())
}
