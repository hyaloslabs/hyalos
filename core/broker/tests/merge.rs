//! Merging (S5.9, ADR-0071): the engine's `merge` message has the broker merge the pull request its
//! manifest's approval names, with the job's token, by the approved method and only at the approved
//! head (GitHub's `sha`). A manifest that merges without an approval, or tainted, is never admitted;
//! nothing the forge answers but the merge commit's id reaches the engine.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyalos_broker::Broker;
use hyalos_broker::egress::Egress;
use hyalos_broker::forge::{Forge, parse_merged};
use hyalos_broker::git::Git;
use hyalos_broker::model::Meter;
use hyalos_broker::sandbox::{Models, Sandboxes};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Approval, Budget, Clock, CommitSha, ForgeToken, JobId, MANIFEST_VERSION, Manifest, ManifestKey,
    MergeMethod, ReasonCode, Role, Timestamp,
};
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const TOKEN: &str = "ghs_merge_test_token";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const MERGED: &str = "abababababababababababababababababababab";

struct Shared(AtomicU64);

impl Clock for Shared {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

fn key() -> ManifestKey {
    ManifestKey::from_seed([21; 32])
}

fn manifest(job: u64) -> Res<Manifest> {
    Ok(Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Deterministic,
        tainted: false,
        repo: "octo/repo".into(),
        branch_prefix: String::new(),
        pr_base: None,
        actions: ["merge".to_string()].into(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 0 },
        deadline: Timestamp::from_millis(10_000),
        approval: Some(Approval {
            event: 3,
            pr: 42,
            head_sha: CommitSha::new(HEAD).ok_or("sha")?,
            method: MergeMethod::Squash,
        }),
        handles: Default::default(),
        result: None,
    })
}

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    authorization: String,
    body: Value,
}

/// GitHub's REST API on loopback, answering every request with the reply it is given.
struct Api {
    port: u16,
    reply: Arc<Mutex<(u16, String)>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Api {
    fn start() -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let merged =
            json!({ "sha": MERGED, "merged": true, "message": "Pull Request successfully merged" });
        let reply = Arc::new(Mutex::new((200, merged.to_string())));
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
    if let Ok(mut seen) = seen.lock() {
        seen.push(Seen {
            method,
            path,
            authorization,
            body,
        });
    }
    let (status, text) = reply.lock().map(|r| r.clone()).unwrap_or_default();
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
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("hy-merge-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

struct Rig {
    broker: Broker,
    clock: Arc<Shared>,
}

fn rig(api: &Api) -> Rig {
    let clock = Arc::new(Shared(AtomicU64::new(100)));
    let meter = Arc::new(Meter::default());
    let sandboxes = Sandboxes {
        models: Arc::new(Models::new(
            Err("model.no_provider"),
            Err("model.no_provider"),
            meter,
        )),
        egress: Egress::public(),
        git: Git::github(),
        forge: Forge::with_base(&format!("http://127.0.0.1:{}", api.port)),
        clock: clock.clone(),
    };
    Rig {
        broker: Broker::with_sandboxes(key().public(), sandboxes)
            .with_forge(Forge::with_base(&format!("http://127.0.0.1:{}", api.port))),
        clock,
    }
}

impl Rig {
    fn ask(&mut self, message: ToBroker) -> Res<Vec<ToEngine>> {
        Ok(self
            .broker
            .handle(message, self.clock.as_ref())?
            .ok_or("stopped")?)
    }

    fn start(&mut self, m: &Manifest, token: Option<&str>) -> Res<Vec<ToEngine>> {
        let socket = dir(&format!("sock-{}", m.job.seq()))?.join("job.sock");
        self.ask(ToBroker::StartJob {
            job: m.job,
            role: m.role,
            manifest: key().sign(m)?.to_bytes()?,
            socket: socket.to_string_lossy().into_owned(),
            forge_token: token
                .map(|t| ForgeToken::new(t, Timestamp::from_millis(99_999)).ok_or("token"))
                .transpose()?,
        })
    }

    fn merge(&mut self, job: JobId) -> Res<Vec<ToEngine>> {
        self.ask(ToBroker::Merge { job })
    }
}

fn refused(job: JobId, code: &str) -> Res<Vec<ToEngine>> {
    Ok(vec![ToEngine::Refused {
        job,
        reason: ReasonCode::new(code).ok_or("code")?,
    }])
}

#[test]
fn an_approved_merge_is_asked_of_github_at_the_approved_head_by_the_approved_method() -> Res {
    let api = Api::start()?;
    let mut rig = rig(&api);
    let m = manifest(4)?;
    assert_eq!(
        rig.start(&m, Some(TOKEN))?,
        vec![ToEngine::Admitted { job: m.job }]
    );
    assert_eq!(
        rig.merge(m.job)?,
        vec![ToEngine::Merged {
            job: m.job,
            sha: CommitSha::new(MERGED).ok_or("sha")?,
        }]
    );
    let seen = api.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "PUT");
    assert_eq!(seen[0].path, "/repos/octo/repo/pulls/42/merge");
    assert_eq!(seen[0].authorization, format!("Bearer {TOKEN}"));
    assert_eq!(
        seen[0].body,
        json!({ "sha": HEAD, "merge_method": "squash" })
    );
    // One merge a job: it is over in the broker now.
    assert_eq!(rig.merge(m.job)?, refused(m.job, "admission.not_live")?);
    assert_eq!(api.seen().len(), 1);
    Ok(())
}

#[test]
fn what_github_refuses_is_a_code_and_ends_the_job() -> Res {
    let api = Api::start()?;
    let mut rig = rig(&api);
    for (n, (status, body, code)) in [
        (
            409,
            r#"{"message":"Head branch was modified. Review and try the merge again."}"#,
            "merge.head_moved",
        ),
        (
            405,
            r#"{"message":"Pull Request is not mergeable"}"#,
            "merge.not_mergeable",
        ),
        (422, r#"{"message":"Validation Failed"}"#, "merge.rejected"),
        (
            403,
            r#"{"message":"Resource not accessible by integration"}"#,
            "forge.upstream_refused",
        ),
        (
            404,
            r#"{"message":"Not Found"}"#,
            "forge.upstream_not_found",
        ),
        (500, "oops", "forge.upstream_status"),
        (
            200,
            &json!({ "sha": MERGED, "merged": false }).to_string(),
            "merge.not_merged",
        ),
        (
            200,
            r#"{"sha":"not a commit","merged":true}"#,
            "forge.malformed",
        ),
        (200, "not json", "forge.malformed"),
    ]
    .into_iter()
    .enumerate()
    {
        api.answer_with(status, body);
        let m = manifest(10 + n as u64)?;
        assert_eq!(
            rig.start(&m, Some(TOKEN))?,
            vec![ToEngine::Admitted { job: m.job }]
        );
        assert_eq!(rig.merge(m.job)?, refused(m.job, code)?, "{status} {body}");
        assert_eq!(rig.merge(m.job)?, refused(m.job, "admission.not_live")?);
    }
    Ok(())
}

#[test]
fn a_merge_without_its_approval_or_tainted_is_not_admitted() -> Res {
    let api = Api::start()?;
    let mut rig = rig(&api);
    let mut no_approval = manifest(4)?;
    no_approval.approval = None;
    let mut tainted = manifest(5)?;
    tainted.tainted = true;
    let mut approval_alone = manifest(6)?;
    approval_alone.actions = ["read".to_string()].into();
    for m in [no_approval, tainted, approval_alone] {
        assert_eq!(
            rig.start(&m, Some(TOKEN))?,
            refused(m.job, "admission.merge_unapproved")?,
            "{m:?}"
        );
        assert_eq!(rig.merge(m.job)?, refused(m.job, "admission.not_live")?);
    }
    // A job that does not merge is never merged for, however it asks.
    let mut read = manifest(7)?;
    read.actions = ["read".to_string()].into();
    read.approval = None;
    assert_eq!(
        rig.start(&read, Some(TOKEN))?,
        vec![ToEngine::Admitted { job: read.job }]
    );
    assert_eq!(
        rig.merge(read.job)?,
        refused(read.job, "merge.not_approved")?
    );
    assert!(api.seen().is_empty());
    Ok(())
}

#[test]
fn no_token_or_past_the_deadline_asks_nothing_of_github() -> Res {
    let api = Api::start()?;
    let mut rig = rig(&api);
    let m = manifest(4)?;
    assert_eq!(
        rig.start(&m, None)?,
        vec![ToEngine::Admitted { job: m.job }]
    );
    assert_eq!(rig.merge(m.job)?, refused(m.job, "forge.no_token")?);
    let m = manifest(5)?;
    assert_eq!(
        rig.start(&m, Some(TOKEN))?,
        vec![ToEngine::Admitted { job: m.job }]
    );
    rig.clock.0.store(20_000, Ordering::SeqCst);
    assert_eq!(rig.merge(m.job)?, refused(m.job, "admission.expired")?);
    assert!(api.seen().is_empty());
    Ok(())
}

#[test]
fn only_merged_and_a_commit_id_are_read_from_the_answer() {
    let sha = CommitSha::new(MERGED);
    assert_eq!(
        parse_merged(
            json!({ "sha": MERGED, "merged": true, "message": "\u{202e}anything" })
                .to_string()
                .as_bytes()
        )
        .ok(),
        sha
    );
    let deep = format!("{}{}", "[".repeat(64), "]".repeat(64));
    for bad in [
        deep.as_str(),
        r#"{"merged":true}"#,
        r#"{"merged":"true","sha":"abababababababababababababababababababab"}"#,
        r#"{"merged":true,"sha":"ABABABABABABABABABABABABABABABABABABABAB"}"#,
    ] {
        assert!(parse_merged(bad.as_bytes()).is_err(), "{bad}");
    }
    assert_eq!(parse_merged(&vec![b' '; 17 * 1024]), Err("forge.too_large"));
}
