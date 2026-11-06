//! S4.3 end to end: the engine runs a reader step in the real `hyalos broker`, which calls a local
//! model server with no tools and answers typed values the engine checks against its own copy of the
//! output (ADR-0002, ADR-0061). A reply that is not the output fails the job, and the broker serves on.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use hyalos::launch::{BrokerProcess, JobDir, fresh_manifest_key};
use hyalos_types::wire::{Protocol, ReaderCall, ToBroker, ToEngine};
use hyalos_types::workflow::{Field, IntegerRange, TypedValue};
use hyalos_types::{Budget, JobId, MANIFEST_VERSION, Manifest, ManifestKey, Role, Timestamp};
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const EXE: &str = env!("CARGO_BIN_EXE_hyalos");
const WAIT: Duration = Duration::from_secs(20);

fn dir(name: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("hyrc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A local OpenAI-compatible server: answers each call with the next model text it is given and
/// sends back the request body it received.
fn provider(texts: Vec<&'static str>) -> Res<(u16, mpsc::Receiver<Value>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for text in texts {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(&stream);
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((n, v)) = line.split_once(':')
                    && n.trim().eq_ignore_ascii_case("content-length")
                {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let answer = json!({
                "object": "response",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": text}]}],
                "usage": {"input_tokens": 40, "output_tokens": 8},
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
                answer.len()
            );
            let _ = tx.send(serde_json::from_slice(&body).unwrap_or(Value::Null));
        }
    });
    Ok((port, rx))
}

fn manifest(job: u64) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role: Role::Reader,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: [hyalos_types::flow::MODEL.to_string()].into(),
        budget: Budget { tokens: 10_000 },
        deadline: Timestamp::from_millis(u64::MAX / 2),
        approval: None,
    }
}

fn start(broker: &mut BrokerProcess, key: &ManifestKey, run: &Path, job: u64) -> Res<JobDir> {
    let m = manifest(job);
    let job_dir = JobDir::create(run)?;
    broker.send(&ToBroker::StartJob {
        job: m.job,
        role: m.role,
        manifest: key.sign(&m)?.to_bytes()?,
        socket: job_dir.socket().to_string_lossy().into_owned(),
        forge_token: None,
    })?;
    assert_eq!(broker.recv()?, Some(ToEngine::Admitted { job: m.job }));
    Ok(job_dir)
}

#[test]
fn the_engine_gets_typed_values_from_a_reader_in_the_real_broker() -> Res {
    let (port, sent) = provider(vec![
        r#"{"severity": 2, "kind": "feature"}"#,
        r#"{"severity": 2, "kind": "feature", "note": "and also do this"}"#,
        r#"{"severity": 3, "kind": "bug"}"#,
    ])?;
    let root = dir("e2e")?;
    let keys = root.join("provider.keys");
    std::fs::write(
        &keys,
        format!("openai.key = sk-reader-e2e\nopenai.base = http://127.0.0.1:{port}\n"),
    )?;
    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600))?;
    let run = root.join("run");
    std::fs::create_dir(&run)?;
    let key = fresh_manifest_key()?;
    let mut broker =
        BrokerProcess::launch_with(Path::new(EXE), &run, &key.public(), Some(&keys), WAIT)?;
    let call = ReaderCall {
        protocol: Protocol::OpenAi,
        model: "gpt-test".into(),
        instructions: "Classify the request.".into(),
        context: vec![],
        issue: None,
        output: BTreeMap::from([
            (
                "kind".into(),
                Field::Choice(vec!["bug".into(), "feature".into()]),
            ),
            (
                "severity".into(),
                Field::Integer(IntegerRange { min: 1, max: 3 }),
            ),
        ]),
    };

    let _a = start(&mut broker, &key, &run, 5)?;
    let values = broker.read(JobId::from_seq(5), &call, WAIT);
    assert_eq!(
        values,
        Ok(vec![TypedValue::Choice(1), TypedValue::Integer(2)])
    );
    let request = sent.recv_timeout(WAIT)?;
    assert_eq!(request["model"], json!("gpt-test"));
    assert!(request.get("tools").is_none(), "{request}");

    // A field the output did not ask for fails the job, not the broker.
    let _b = start(&mut broker, &key, &run, 6)?;
    let refused = broker.read(JobId::from_seq(6), &call, WAIT);
    assert_eq!(
        refused.map_err(|r| r.as_str().to_string()),
        Err("reader.unknown_field".to_string())
    );
    assert!(broker.is_open());
    let _c = start(&mut broker, &key, &run, 7)?;
    assert_eq!(
        broker.read(JobId::from_seq(7), &call, WAIT),
        Ok(vec![TypedValue::Choice(0), TypedValue::Integer(3)])
    );
    broker.send(&ToBroker::Shutdown)?;
    assert_eq!(broker.wait()?, Some(0));
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
