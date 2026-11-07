//! `hyalos mcp` (S5.1, ADR-0067): the binary on stdio, with `list_tasks` reading a real log with the
//! public key alone, and `--listen` refusing anything but loopback. Every call needs a credential the
//! operator issued with `hyalos agent issue` and has not revoked (S5.8, ADR-0068). `start_job` and
//! the run's task, with the Tasks extension and without it (S5.2, ADR-0069).

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
        // The operator's approval key, which every credential issued or revoked is signed with.
        let out = self.signed(&["operator", "init"])?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
        }
        Ok(())
    }

    /// A command that records the operator's decision, the passphrase given on stdin (ADR-0070).
    fn signed(&self, args: &[&str]) -> Res<std::process::Output> {
        let mut child = self
            .command(args)
            .arg("--passphrase-stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "the operator's own passphrase");
        }
        Ok(child.wait_with_output()?)
    }

    /// Issues a credential with the binary, as the operator does, and returns it.
    fn issue(&self, name: &str) -> Res<String> {
        let out = self.signed(&["agent", "issue", name])?;
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
        let out = self.signed(&[&["agent"], args].concat())?;
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
    let out = m.signed(&["agent", "issue", "claude-code"]).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already in force"));
    let out = m.signed(&["agent", "revoke", "99"]).unwrap();
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

fn declaring(id: u64, method: &str, params: Value) -> Value {
    let mut message = request(id, method, params);
    message["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] =
        json!({"extensions": {"io.modelcontextprotocol/tasks": {}}});
    message
}

fn text(answer: &Value) -> String {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn events(m: &Machine) -> Res<usize> {
    let log = hyalos_log::Log::open(&m.root.join("data/events.db"), LogKey::from_seed(SEED))?;
    Ok(log.events()?.len())
}

#[test]
fn start_job_takes_only_identifiers_and_says_why_a_runner_could_not_start() {
    let m = Machine::new("start").unwrap();
    m.install_key().unwrap();
    let credential = m.issue("claude-code").unwrap();
    let before = events(&m).unwrap();
    let call = |id, arguments: Value| {
        declaring(
            id,
            "tools/call",
            json!({"name": "start_job", "arguments": arguments}),
        )
    };
    let answers = m
        .stdio(
            Some(&credential),
            &[
                request(1, "server/discover", json!({})),
                call(
                    2,
                    json!({"repo": "octo/repo", "issue_number": 7, "issue": "do it"}),
                ),
                call(3, json!({"repo": "octo/repo; rm", "issue_number": 7})),
                call(4, json!({"repo": "octo/repo", "issue_number": 0})),
                call(
                    5,
                    json!({"repo": "octo/repo", "issue_number": 7, "base": "../main"}),
                ),
                // Well formed, but this machine has no GitHub App: the runner says so and stops.
                call(6, json!({"repo": "octo/repo", "issue_number": 7})),
            ],
        )
        .unwrap();
    assert_eq!(
        answers[0]["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/tasks"],
        json!({})
    );
    for answer in &answers[1..] {
        assert_eq!(answer["result"]["isError"], true, "{answer}");
        assert_eq!(answer["result"]["resultType"], "complete", "{answer}");
    }
    assert!(text(&answers[1]).contains("only repo and issue_number"));
    // The base branch is the operator's: a branch name could carry words to the actor.
    assert!(text(&answers[4]).contains("only repo and issue_number"));
    let refused = text(&answers[5]);
    assert!(refused.starts_with("the job did not start: "), "{refused}");
    // Nothing was written, and the runner's files are gone.
    assert_eq!(events(&m).unwrap(), before);
    let left: Vec<_> = std::fs::read_dir(m.root.join("data/runs"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_runs_task_is_polled_with_the_tasks_extension_or_its_plain_tools_and_reads_the_same() {
    use hyalos_types::TypedValue::{Bool, Choice};
    let m = Machine::new("poll").unwrap();
    m.install_key().unwrap();
    let credential = m.issue("claude-code").unwrap();
    let args: Vec<String> = ["--repo", "octo/repo", "--issue-number", "7"]
        .map(String::from)
        .to_vec();
    let asked = hyalos::run_cmd::parse(&args).unwrap();
    let mut engine = Engine::open(&m.root.join("data/events.db"), LogKey::from_seed(SEED)).unwrap();
    // After the credential's issue, which the binary stamped with the real time.
    let mut at = 4_000_000_000_000;
    let mut write = |engine: &mut Engine, events: Vec<Event>| {
        at += 1;
        let events: Vec<_> = events.into_iter().map(|e| (Origin::Engine, e)).collect();
        engine
            .submit_all(Timestamp::from_millis(at), &events)
            .unwrap()
    };
    let task = TaskId::from_seq(
        write(
            &mut engine,
            vec![Event::TaskCreated {
                request: hyalos::run_cmd::request_digest(&asked),
            }],
        )
        .seq,
    );
    // A task the server did not start: no run document.
    let stranger = write(
        &mut engine,
        vec![Event::TaskCreated {
            request: Digest::from_bytes([9; 32]),
        }],
    )
    .seq
    .to_string();
    let runs = m.root.join("data/runs");
    std::fs::create_dir_all(&runs).unwrap();
    std::fs::write(
        runs.join(format!("task-{}.json", task.seq())),
        serde_json::to_vec(&asked.stored()).unwrap(),
    )
    .unwrap();
    // This test is its runner: it holds the lock, so the server starts none.
    let lock = std::fs::File::create(runs.join(format!("task-{}.lock", task.seq()))).unwrap();
    lock.lock().unwrap();
    let mut step = |engine: &mut Engine, substrate: &str, values, reason: &str| {
        let started = write(
            engine,
            vec![Event::JobStarted {
                task,
                substrate: SubstrateName::new(substrate).unwrap(),
                credentials: CredentialTier::BrokerHeld,
                attestation: Attestation::no_hardware_root(),
            }],
        );
        let job = JobId::from_seq(started.seq);
        write(
            engine,
            vec![
                Event::JobValues { job, values },
                Event::JobEnded {
                    job,
                    outcome: JobOutcome::Succeeded,
                    reason: ReasonCode::new(reason).unwrap(),
                },
            ],
        );
    };
    step(
        &mut engine,
        "broker",
        vec![Bool(true), Bool(false), Choice(2), Choice(0)],
        "step.answered",
    );

    let id = task.seq().to_string();
    let polls = |answers_for: &str| {
        m.stdio(
            Some(&credential),
            &[
                declaring(1, "tasks/get", json!({"taskId": answers_for})),
                request(
                    2,
                    "tools/call",
                    json!({"name": "job_status", "arguments": {"task": answers_for}}),
                ),
                request(
                    3,
                    "tools/call",
                    json!({"name": "job_result", "arguments": {"task": answers_for}}),
                ),
                request(4, "tasks/get", json!({"taskId": answers_for})),
            ],
        )
        .unwrap()
    };
    let working = polls(&id);
    let got = &working[0]["result"];
    assert_eq!(got["status"], "working", "{got}");
    assert_eq!(got["taskId"], id.as_str());
    assert_eq!(got["statusMessage"], "1 of 3 steps done");
    assert_eq!(got["pollIntervalMs"], 5000);
    assert!(got["ttlMs"].is_null());
    assert!(got.get("result").is_none());
    // Without the extension: the same task through the plain tools, and tasks/get refused.
    let plain = &working[1]["result"]["structuredContent"];
    assert_eq!(plain["status"], "working", "{plain}");
    assert_eq!(plain["taskId"], id.as_str());
    assert_eq!(working[2]["result"]["isError"], true);
    assert!(text(&working[2]).contains("still working"));
    assert_eq!(working[3]["error"]["code"], -32021);

    // A runner that stopped with an error is never started again by the server: the task says it
    // waits for the operator, and no resume wrote to its log.
    lock.unlock().unwrap();
    let failed = runs.join(format!("task-{}.failed", task.seq()));
    std::fs::write(&failed, "hyalos: the log could not be written\n").unwrap();
    let waiting = m
        .stdio(
            Some(&credential),
            &[declaring(1, "tasks/get", json!({"taskId": id}))],
        )
        .unwrap();
    let message = waiting[0]["result"]["statusMessage"].as_str().unwrap_or("");
    assert!(message.contains("waits for the operator"), "{message}");
    assert!(!runs.join(format!("task-{}.log", task.seq())).exists());
    std::fs::remove_file(&failed).unwrap();
    lock.lock().unwrap();

    // A cancel is asked for in a file the runner watches.
    let cancelled = m
        .stdio(
            Some(&credential),
            &[declaring(1, "tasks/cancel", json!({"taskId": id}))],
        )
        .unwrap();
    assert_eq!(cancelled[0]["result"]["resultType"], "complete");
    let cancel = runs.join(format!("task-{}.cancel", task.seq()));
    assert!(cancel.exists());
    std::fs::remove_file(&cancel).unwrap();

    step(
        &mut engine,
        "broker",
        vec![Bool(false), Choice(2)],
        "step.answered",
    );
    step(
        &mut engine,
        "local-process",
        vec![Choice(0), Bool(true)],
        "step.submitted",
    );
    write(&mut engine, vec![Event::TaskCompleted { task }]);
    let done = polls(&id);
    let got = &done[0]["result"];
    assert_eq!(got["status"], "completed", "{got}");
    // The harness read the issue: its values are marked untrusted, in the structured result and in
    // the text a client without structured content reads (ADR-0005, ADR-0010).
    let result = json!({
        "task": task.seq(),
        "outcome": "succeeded",
        "result": {"outcome": "pr_opened", "tests_added": true},
        "provenance": "model_from_tainted",
        "untrusted": true,
    });
    assert_eq!(got["result"]["structuredContent"], result);
    assert_eq!(got["result"]["isError"], false);
    assert_eq!(
        done[1]["result"]["structuredContent"]["status"],
        "completed"
    );
    assert_eq!(done[2]["result"]["structuredContent"], result);
    for plain in [&got["result"], &done[2]["result"]] {
        let text = plain["content"][0]["text"].as_str().unwrap_or("");
        assert!(text.contains(hyalos::mcp::UNTRUSTED_NOTICE), "{text}");
        assert!(
            text.contains(
                "<untrusted-data provenance=\"model_from_tainted\">\n{\"outcome\":\"pr_opened\",\"tests_added\":true}\n</untrusted-data>"
            ),
            "{text}"
        );
    }
    // job_status shows the whole task as JSON text, the marked result inside it.
    let status: Value = serde_json::from_str(
        done[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or(""),
    )
    .unwrap();
    let inner = status["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(inner.contains(hyalos::mcp::UNTRUSTED_NOTICE), "{inner}");
    assert!(
        inner.contains("<untrusted-data provenance=\"model_from_tainted\">"),
        "{inner}"
    );
    // No answer holds the agent's credential or the engine's key (its seed, as engine.key holds it).
    let seed: String = SEED.iter().map(|b| format!("{b:02x}")).collect();
    for answer in working.iter().chain(&done) {
        let answer = answer.to_string();
        assert!(!answer.contains(&credential), "{answer}");
        assert!(!answer.contains(&seed), "{answer}");
    }
    // Cancelling a finished task changes nothing.
    let again = m
        .stdio(
            Some(&credential),
            &[declaring(1, "tasks/cancel", json!({"taskId": id}))],
        )
        .unwrap();
    assert_eq!(again[0]["result"]["resultType"], "complete");
    assert!(!cancel.exists());

    // Not one of the server's runs, or not a task at all.
    for other in [stranger.as_str(), "99", "01", "x"] {
        let answers = polls(other);
        assert_eq!(answers[0]["error"]["code"], -32602, "{other}");
        assert_eq!(answers[1]["result"]["isError"], true, "{other}");
    }
    drop(lock);
}

/// A runner stopped before it answered may already have recorded its task (ADR-0069): the server
/// adopts the newest open task with the request's digest created since the call began, and writes its
/// request document if the runner had not, so the run resumes like any other.
#[test]
fn a_task_a_stopped_runner_recorded_is_adopted_not_lost() {
    let m = Machine::new("adopt").unwrap();
    m.install_key().unwrap();
    let args: Vec<String> = ["--repo", "octo/repo", "--issue-number", "7"]
        .map(String::from)
        .to_vec();
    let mut asked = hyalos::run_cmd::parse(&args).unwrap();
    asked.nonce = Some("0123456789abcdef0123456789abcdef".into());
    let mut engine = Engine::open(&m.root.join("data/events.db"), LogKey::from_seed(SEED)).unwrap();
    let created = |engine: &mut Engine, at, request| {
        engine
            .submit(
                Timestamp::from_millis(at),
                Origin::Engine,
                Event::TaskCreated { request },
            )
            .unwrap()
            .seq
    };
    let digest = hyalos::run_cmd::request_digest(&asked);
    let earlier = created(&mut engine, 1_000, digest);
    created(&mut engine, 2_000, Digest::from_bytes([4; 32]));
    let recorded = created(&mut engine, 3_000, digest);
    let runs = hyalos::runs::Runs::at(m.root.join("data/runs"));
    std::fs::create_dir_all(runs.dir()).unwrap();
    let launcher = hyalos::runs::Launcher {
        runs: runs.clone(),
        exe: PathBuf::from("/nonexistent"),
        wait: std::time::Duration::from_secs(1),
    };
    let state = engine.state().clone();
    let since = Timestamp::from_millis(2_500);
    let adopted = launcher.adopt(&state, &asked, since).unwrap();
    assert_eq!(adopted.seq(), recorded);
    assert!(runs.request(&state, adopted).is_some());
    assert!(runs.request(&state, TaskId::from_seq(earlier)).is_none());
    // Nothing of another call's request, though it asks the same, or from before the call.
    let mut again = asked.clone();
    again.nonce = Some("fedcba9876543210fedcba9876543210".into());
    assert!(launcher.adopt(&state, &again, since).is_none());
    assert!(
        launcher
            .adopt(&state, &asked, Timestamp::from_millis(3_001))
            .is_none()
    );

    // A held run stops compaction, which would change the log under its engine.
    let lock = std::fs::File::create(runs.file(adopted, "lock")).unwrap();
    lock.lock().unwrap();
    assert_eq!(runs.held(), [recorded]);
    let out = m.command(&["log", "compact"]).output().unwrap();
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains(&format!("a run is in progress (task {recorded})")),
        "{said}"
    );
    drop(lock);
    // Another test's child, forked while the lock was open, shares it until it execs: wait for that.
    let free = (0..100).any(|_| {
        let free = runs.held().is_empty();
        if !free {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        free
    });
    assert!(free);
    assert!(runs.none_running().is_ok());
}

/// S5.3: what waits for the operator is shown to the driving agent as read-only details and the
/// command the operator runs, as structured content and as plain text, and nothing it can call
/// decides it (ADR-0010, ADR-0012, ADR-0074).
#[test]
fn approvals_are_shown_with_the_operators_commands_and_the_server_cannot_decide_them() {
    use hyalos_types::{Gate, HoldReason, Sensitive, Stakes};
    let m = Machine::new("approvals").unwrap();
    m.install_key().unwrap();
    let credential = m.issue("claude-code").unwrap();
    let args: Vec<String> = ["--repo", "octo/repo", "--issue-number", "7"]
        .map(String::from)
        .to_vec();
    let asked = hyalos::run_cmd::parse(&args).unwrap();
    let mut engine = Engine::open(&m.root.join("data/events.db"), LogKey::from_seed(SEED)).unwrap();
    // After the credential's issue and before the operator's approval, which the binary stamps
    // with the real time.
    let mut at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut write = |engine: &mut Engine, events: Vec<Event>| {
        at += 1;
        let events: Vec<_> = events.into_iter().map(|e| (Origin::Engine, e)).collect();
        engine
            .submit_all(Timestamp::from_millis(at), &events)
            .unwrap()
    };
    // A run the server started, whose job stopped for the content gate at irreversible stakes.
    let run = TaskId::from_seq(
        write(
            &mut engine,
            vec![Event::TaskCreated {
                request: hyalos::run_cmd::request_digest(&asked),
            }],
        )
        .seq,
    );
    let runs = m.root.join("data/runs");
    std::fs::create_dir_all(&runs).unwrap();
    std::fs::write(
        runs.join(format!("task-{}.json", run.seq())),
        serde_json::to_vec(&asked.stored()).unwrap(),
    )
    .unwrap();
    let lock = std::fs::File::create(runs.join(format!("task-{}.lock", run.seq()))).unwrap();
    lock.lock().unwrap();
    let job = JobId::from_seq(
        write(
            &mut engine,
            vec![Event::JobStarted {
                task: run,
                substrate: SubstrateName::new("local-process").unwrap(),
                credentials: CredentialTier::BrokerHeld,
                attestation: Attestation::no_hardware_root(),
            }],
        )
        .seq,
    );
    let held = write(
        &mut engine,
        vec![
            Event::JobEnded {
                job,
                outcome: JobOutcome::Held,
                reason: ReasonCode::new("content.pack_unreadable").unwrap(),
            },
            Event::HoldOpened {
                task: run,
                gate: Gate::Content,
                inputs: Digest::from_bytes([3; 32]),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible().touching(Sensitive::Credentials),
            },
        ],
    );
    let first = held.seq;
    // A second task, held at routine stakes after a gate timed out.
    let other = TaskId::from_seq(
        write(
            &mut engine,
            vec![Event::TaskCreated {
                request: Digest::from_bytes([4; 32]),
            }],
        )
        .seq,
    );
    let second = write(
        &mut engine,
        vec![Event::HoldOpened {
            task: other,
            gate: Gate::Egress,
            inputs: Digest::from_bytes([5; 32]),
            reason: HoldReason::TimedOut,
            stakes: Stakes::routine(),
        }],
    )
    .seq;
    drop(engine);

    let calls = |messages: &[Value]| m.stdio(Some(&credential), messages).unwrap();
    let before = events(&m).unwrap();
    let answers = calls(&[
        request(1, "tools/list", json!({})),
        request(
            2,
            "tools/call",
            json!({"name": "list_approvals", "arguments": {}}),
        ),
        request(
            3,
            "tools/call",
            json!({"name": "job_status", "arguments": {"task": run.seq().to_string()}}),
        ),
        request(
            4,
            "tools/call",
            json!({"name": "list_approvals", "arguments": {"limit": 0}}),
        ),
    ]);
    // Asking changed nothing in the log.
    assert_eq!(events(&m).unwrap(), before);

    // No tool decides: the server offers only these, and none releases, cancels or merges.
    let names: Vec<&str> = answers[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "list_tasks",
            "start_job",
            "job_status",
            "job_result",
            "list_approvals"
        ]
    );

    let listed = &answers[1]["result"];
    assert_eq!(listed["isError"], false, "{listed}");
    let got = &listed["structuredContent"];
    assert_eq!(got["total"], 2, "{got}");
    // Newest first.
    assert_eq!(
        got["approvals"][0],
        json!({
            "hold": second,
            "task": other.seq(),
            "gate": "egress",
            "reason": "timed_out",
            "inputs": Digest::from_bytes([5; 32]).to_string(),
            "tier": "routine",
            "touches": [],
            "may_decide": ["policy", "operator"],
            "opened_at_ms": at,
            "approve": format!("hyalos approve {second}"),
            "cancel": format!("hyalos cancel {}", other.seq()),
        })
    );
    let held_one = &got["approvals"][1];
    assert_eq!(held_one["hold"], first);
    assert_eq!(held_one["tier"], "irreversible");
    assert_eq!(held_one["touches"], json!(["credentials"]));
    assert_eq!(held_one["may_decide"], json!(["operator"]));
    // The same details as sentences, for a client that reads only text.
    let plain = text(&answers[1]);
    assert!(
        plain.starts_with("2 approvals are waiting for the operator."),
        "{plain}"
    );
    assert!(
        plain.contains("Nothing in this server can release a hold"),
        "{plain}"
    );
    assert!(
        plain.contains(&format!(
            "Hold {first} on task {}: the content gate could not decide, so its job stopped. Stakes: irreversible, touching credentials. Who may release it: operator.",
            run.seq()
        )),
        "{plain}"
    );
    assert!(
        plain.contains(&format!("  To release it: hyalos approve {first}\n")),
        "{plain}"
    );
    assert!(
        plain.contains(&format!(
            "  To cancel the task instead: hyalos cancel {}",
            run.seq()
        )),
        "{plain}"
    );
    assert!(
        plain.contains("the egress gate did not answer in time"),
        "{plain}"
    );
    assert!(!plain.trim_start().starts_with('{'), "{plain}");

    // The held run shows its own hold and command, in job_status's task and its result's text.
    let status = &answers[2]["result"]["structuredContent"];
    assert_eq!(status["status"], "completed", "{status}");
    assert_eq!(
        status["statusMessage"],
        format!("held for the operator's decision (hyalos approve {first})")
    );
    let result = &status["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["outcome"], "held");
    assert_eq!(result["structuredContent"]["approvals"], json!([held_one]));
    let inner = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        inner.starts_with(&format!(
            "Task {} is held: its job stopped for a decision (content.pack_unreadable).",
            run.seq()
        )),
        "{inner}"
    );
    assert!(
        inner.contains(&format!("hyalos approve {first}")),
        "{inner}"
    );
    assert_eq!(answers[3]["result"]["isError"], true);

    // Once the operator decides at their own terminal, the hold is no longer listed.
    let out = m.signed(&["approve", &second.to_string()]).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = calls(&[request(
        1,
        "tools/call",
        json!({"name": "list_approvals", "arguments": {}}),
    )]);
    let got = &after[0]["result"]["structuredContent"];
    assert_eq!(got["total"], 1, "{got}");
    assert_eq!(got["approvals"][0]["hold"], first);
    assert!(text(&after[0]).starts_with("1 approval is waiting for the operator."));
    drop(lock);
}
