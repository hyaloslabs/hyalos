//! `hyalos mcp` (S5.1, ADR-0067): the binary on stdio, with `list_tasks` reading a real log with the
//! public key alone, and `--listen` refusing anything but loopback. Every call needs a credential the
//! operator issued with `hyalos agent issue` and has not revoked (S5.8, ADR-0068).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
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

    /// The engine's private key too, which `hyalos agent` signs with. The MCP server never reads it.
    fn install_key(&self) -> Res {
        self.publish_key()?;
        std::fs::set_permissions(
            self.root.join("config"),
            std::fs::Permissions::from_mode(0o700),
        )?;
        let private = self.root.join("config/engine.key");
        let hex: String = SEED.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&private, format!("hyalos-engine-key-v1:{hex}\n"))?;
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    /// Issues a credential with the binary, as the operator does, and returns it.
    fn issue(&self, name: &str) -> Res<String> {
        let out = self.command(&["agent", "issue", name]).output()?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
        }
        let credential = String::from_utf8(out.stdout)?.trim().to_string();
        if !credential.starts_with("hyalos-agent-") {
            return Err(format!("not a credential: {credential}").into());
        }
        Ok(credential)
    }

    fn agent(&self, args: &[&str]) -> Res<String> {
        let out = self.command(&[&["agent"], args].concat()).output()?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
        }
        Ok(String::from_utf8(out.stdout)?)
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

    /// Sends each message on stdin to a server started with `credential` and returns one parsed
    /// answer per line of stdout.
    fn stdio(&self, credential: Option<&str>, messages: &[Value]) -> Res<Vec<Value>> {
        let mut command = self.command(&["mcp"]);
        match credential {
            Some(c) => command.env("HYALOS_AGENT_CREDENTIAL", c),
            None => command.env_remove("HYALOS_AGENT_CREDENTIAL"),
        };
        let mut child = command
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
    m.install_key().unwrap();
    m.log_two_tasks().unwrap();
    let credential = m.issue("claude-code").unwrap();
    let answers = m
        .stdio(
            Some(&credential),
            &[
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
            ],
        )
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
fn without_a_key_a_log_or_a_credential_in_force_the_server_does_not_start_and_says_why() {
    let m = Machine::new("missing").unwrap();
    let start = |credential: Option<&str>| m.stdio(credential, &[]).unwrap_err().to_string();
    assert!(start(Some("x")).contains("public key"));
    m.install_key().unwrap();
    assert!(start(Some("x")).contains("no event log"));
    let credential = m.issue("claude-code").unwrap();
    assert!(start(None).contains("HYALOS_AGENT_CREDENTIAL"));
    assert!(start(Some("hyalos-agent-00")).contains("not issued, or it was revoked"));
    m.stdio(Some(&credential), &[]).unwrap();
    // The log the credential is in must check out against engine.pub.
    std::fs::remove_file(m.root.join("data/events.db")).unwrap();
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
    assert!(start(Some(&credential)).contains("does not check out"));
}

#[test]
fn list_tasks_refuses_a_limit_out_of_range() {
    let m = Machine::new("limit").unwrap();
    m.install_key().unwrap();
    let credential = m.issue("claude-code").unwrap();
    for bad in [json!(0), json!(101), json!("5")] {
        let call = request(
            1,
            "tools/call",
            json!({"name": "list_tasks", "arguments": {"limit": bad}}),
        );
        let answer = &m.stdio(Some(&credential), &[call]).unwrap()[0];
        assert_eq!(answer["result"]["isError"], true);
        assert!(
            answer["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("limit must be"),
            "{answer}"
        );
    }
}

/// One HTTP POST of `body` to the server at `port`, with `credential` as its bearer token.
fn post(port: u16, credential: Option<&str>, body: &Value) -> Res<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    let body = body.to_string();
    let auth = credential
        .map(|c| format!("Authorization: Bearer {c}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2026-07-28\r\nMcp-Method: tools/call\r\nMcp-Name: list_tasks\r\n{auth}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )?;
    let mut out = String::new();
    stream.read_to_string(&mut out)?;
    let status = out
        .get(9..12)
        .and_then(|s| s.parse().ok())
        .ok_or("no status line")?;
    Ok((status, out))
}

#[test]
fn over_http_a_credential_is_admitted_until_the_operator_revokes_it_and_then_the_next_call_fails() {
    let m = Machine::new("revoke").unwrap();
    m.install_key().unwrap();
    m.log_two_tasks().unwrap();
    let first = m.issue("claude-code").unwrap();
    let second = m.issue("hermes").unwrap();
    let mut server = m
        .command(&["mcp", "--listen", "127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The server says where it listens on stderr.
    let mut stderr = BufReader::new(server.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    let port: u16 = line
        .rsplit_once(':')
        .and_then(|(_, rest)| rest.trim().strip_suffix("/mcp"))
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port in {line:?}"));
    let call = request(
        1,
        "tools/call",
        json!({"name": "list_tasks", "arguments": {}}),
    );

    let (status, body) = post(port, Some(&first), &call).unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"total\":2"), "{body}");
    let (status, body) = post(port, None, &call).unwrap();
    assert_eq!(status, 401, "{body}");
    assert!(body.contains("WWW-Authenticate: Bearer"), "{body}");

    // Revoked while the server runs: the very next call with it is refused, the other still works.
    let listed = m.agent(&["list"]).unwrap();
    let id = listed
        .lines()
        .find(|l| l.contains("claude-code"))
        .and_then(|l| l.split('\t').next())
        .unwrap()
        .to_string();
    assert!(m.agent(&["revoke", &id]).unwrap().contains("revoked agent"));
    let (status, body) = post(port, Some(&first), &call).unwrap();
    assert_eq!(status, 401, "{body}");
    assert!(body.contains("-32030"), "{body}");
    let (status, _) = post(port, Some(&second), &call).unwrap();
    assert_eq!(status, 200);
    assert!(m.agent(&["list"]).unwrap().contains("revoked at"));
    server.kill().unwrap();
    server.wait().unwrap();
}

#[test]
fn the_log_holds_the_credentials_digest_never_the_credential() {
    let m = Machine::new("digest").unwrap();
    m.install_key().unwrap();
    let credential = m.issue("claude-code").unwrap();
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(m.root.join("data")).unwrap() {
        bytes.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    let secret = credential.strip_prefix("hyalos-agent-").unwrap();
    assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
    assert!(!bytes.windows(13).any(|w| w == b"hyalos-agent-"));
}

#[test]
fn agent_commands_refuse_bad_names_ids_and_reasons() {
    let m = Machine::new("agent-usage").unwrap();
    m.install_key().unwrap();
    for args in [
        &["agent", "issue", "Claude Code"][..],
        &["agent", "issue", "claude", "--reason", "Free text"],
        &["agent", "revoke", "one"],
        &["agent", "rotate"],
    ] {
        let out = m.command(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    m.issue("claude-code").unwrap();
    // A name in force is not issued twice, and an unknown agent is not revoked.
    let out = m
        .command(&["agent", "issue", "claude-code"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already in force"));
    let out = m.command(&["agent", "revoke", "99"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
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
