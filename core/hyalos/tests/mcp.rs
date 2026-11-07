//! `hyalos mcp` (S5.1, ADR-0067): the binary on stdio, with `list_tasks` reading a real log with the
//! public key alone, and `--listen` refusing anything but loopback.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use hyalos_engine::Engine;
use hyalos_log::LogKey;
use hyalos_types::{
    Attestation, CredentialTier, Digest, Event, JobId, JobOutcome, Origin, ReasonCode,
    SubstrateName, TaskId, Timestamp,
};
use serde_json::{Value, json};

const SEED: [u8; 32] = [11; 32];

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Machine {
    root: PathBuf,
}

impl Machine {
    fn new(name: &str) -> Res<Self> {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data"))?;
        std::fs::create_dir_all(root.join("config"))?;
        Ok(Self { root })
    }

    fn publish_key(&self) -> Res {
        let public = self.root.join("config/engine.pub");
        let key = LogKey::from_seed(SEED).public().to_hex();
        std::fs::write(&public, format!("{key}\n"))?;
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o444))?;
        Ok(())
    }

    /// Two tasks: the first with a job that succeeded, the second with one still running.
    fn log_two_tasks(&self) -> Res {
        let mut engine = Engine::open(&self.root.join("data/events.db"), LogKey::from_seed(SEED))?;
        let substrate = SubstrateName::new("container.podman").ok_or("substrate name")?;
        let started = |task| Event::JobStarted {
            task,
            substrate: substrate.clone(),
            credentials: CredentialTier::BrokerHeld,
            attestation: Attestation::no_hardware_root(),
        };
        let events = [
            Event::TaskCreated {
                request: Digest::from_bytes([1; 32]),
            },
            started(TaskId::from_seq(1)),
            Event::JobEnded {
                job: JobId::from_seq(2),
                outcome: JobOutcome::Succeeded,
                reason: ReasonCode::new("step.submitted").ok_or("reason code")?,
            },
            Event::TaskCreated {
                request: Digest::from_bytes([2; 32]),
            },
            started(TaskId::from_seq(4)),
        ];
        for (at, event) in (1_000..).zip(events) {
            engine.submit(Timestamp::from_millis(at), Origin::Engine, event)?;
        }
        Ok(())
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hyalos"));
        command
            .args(args)
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("HOME")
            .env("HYALOS_DATA_DIR", self.root.join("data"))
            .env("HYALOS_CONFIG_DIR", self.root.join("config"));
        command
    }

    /// Sends each message on stdin and returns one parsed answer per line of stdout.
    fn stdio(&self, messages: &[Value]) -> Res<Vec<Value>> {
        let mut child = self
            .command(&["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        for message in messages {
            writeln!(stdin, "{message}")?;
        }
        drop(stdin);
        let out = child.wait_with_output()?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
        }
        let mut answers = Vec::new();
        for line in String::from_utf8(out.stdout)?.lines() {
            answers.push(serde_json::from_str(line)?);
        }
        Ok(answers)
    }
}

fn request(id: u64, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[test]
fn over_stdio_list_tasks_reads_the_log_newest_first_with_each_jobs_status() {
    let m = Machine::new("list").unwrap();
    m.publish_key().unwrap();
    m.log_two_tasks().unwrap();
    let answers = m
        .stdio(&[
            request(1, "server/discover", json!({})),
            request(2, "tools/list", json!({})),
            request(
                3,
                "tools/call",
                json!({"name": "list_tasks", "arguments": {}}),
            ),
            request(
                4,
                "tools/call",
                json!({"name": "list_tasks", "arguments": {"limit": 1}}),
            ),
        ])
        .unwrap();
    assert_eq!(answers.len(), 4);
    assert_eq!(
        answers[0]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "hyalos"
    );
    assert_eq!(answers[1]["result"]["tools"][0]["name"], "list_tasks");
    let listed = &answers[2]["result"];
    assert_eq!(listed["isError"], false);
    assert_eq!(
        listed["structuredContent"],
        json!({"total": 2, "tasks": [
            {"task": 4, "status": "open", "created_at_ms": 1003, "jobs": [{"job": 5, "status": "running"}]},
            {"task": 1, "status": "open", "created_at_ms": 1000, "jobs": [{"job": 2, "status": "succeeded"}]},
        ]})
    );
    assert_eq!(
        answers[3]["result"]["structuredContent"]["tasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn list_tasks_says_what_is_missing_and_refuses_a_log_another_key_signed() {
    let m = Machine::new("missing").unwrap();
    let call = |limit: Value| {
        request(
            1,
            "tools/call",
            json!({"name": "list_tasks", "arguments": {"limit": limit}}),
        )
    };
    let text = |answer: &Value| {
        answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let answer = &m.stdio(&[call(json!(5))]).unwrap()[0];
    assert_eq!(answer["result"]["isError"], true);
    assert!(text(answer).contains("public key"), "{answer}");
    m.publish_key().unwrap();
    assert!(text(&m.stdio(&[call(json!(5))]).unwrap()[0]).contains("no event log"));
    // A log signed by another key does not check out against engine.pub.
    let other = Engine::open(&m.root.join("data/events.db"), LogKey::from_seed([12; 32]));
    other
        .unwrap()
        .submit(
            Timestamp::from_millis(1),
            Origin::Engine,
            Event::TaskCreated {
                request: Digest::from_bytes([3; 32]),
            },
        )
        .unwrap();
    assert!(text(&m.stdio(&[call(json!(5))]).unwrap()[0]).contains("does not check out"));
    for bad in [json!(0), json!(101), json!("5")] {
        assert!(text(&m.stdio(&[call(bad)]).unwrap()[0]).contains("limit must be"));
    }
}

#[test]
fn listen_takes_a_loopback_address_only_and_bad_usage_is_exit_2() {
    let m = Machine::new("listen").unwrap();
    let out = m
        .command(&["mcp", "--listen", "0.0.0.0:8765"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("loopback"));
    let out = m.command(&["mcp", "--port", "1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
