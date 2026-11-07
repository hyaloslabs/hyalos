//! `hyalos mcp [--listen <loopback address:port>]`: the engine's MCP server (ADR-0010, ADR-0067).
//!
//! On stdio by default, which is how a client such as Claude Code starts it; with `--listen`, on
//! Streamable HTTP at `http://<address:port>/mcp`, loopback only. Its one tool so far is `list_tasks`,
//! which reads the event log with the engine's public key alone and returns ids and statuses, never
//! text from a task. Starting work and approvals come in later stories (S5.2, S5.4).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use hyalos_engine::{TaskStatus, read_state};
use hyalos_log::PublicKey;
use hyalos_types::JobOutcome;
use serde_json::{Map, Value, json};

use crate::mcp::{self, Called, Progress, Server, Tool, Tools};
use crate::paths;

const USAGE: &str = "usage: hyalos mcp [--listen <loopback address:port>]
  serves the engine's MCP server on stdio, or with --listen on http://<address:port>/mcp (loopback
  only). It reads the event log with the engine's public key and writes nothing";

const INSTRUCTIONS: &str = "Hyalos runs agent jobs contained, within a signed manifest. list_tasks shows the tasks in its event log and the state of each one's jobs.";

/// Most tasks `list_tasks` returns.
pub const MAX_TASKS: u64 = 100;

pub fn run(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let tools = match (paths::log_file(&env), paths::engine_public_key(&env)) {
        (Ok(log), Ok(public_key)) => EngineTools::new(log, public_key),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    let server = Server::new(tools, "hyalos", env!("CARGO_PKG_VERSION"), INSTRUCTIONS);
    let served = match args {
        [] => mcp::stdio::serve(&server, std::io::stdin().lock(), std::io::stdout().lock()),
        [flag, addr] if flag == "--listen" => match mcp::http::bind(addr) {
            Ok(listener) => {
                eprintln!("hyalos: MCP server on http://{addr}{}", mcp::http::PATH);
                mcp::http::serve(listener, Arc::new(server))
            }
            Err(e) => {
                eprintln!("hyalos: {e}");
                return ExitCode::from(1);
            }
        },
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hyalos: the MCP server stopped: {e}");
            ExitCode::from(1)
        }
    }
}

/// The engine's tools.
pub struct EngineTools {
    log: PathBuf,
    public_key: PathBuf,
    tools: Vec<Tool>,
}

impl EngineTools {
    pub fn new(log: PathBuf, public_key: PathBuf) -> Self {
        let list_tasks = Tool {
            name: "list_tasks",
            title: "List tasks",
            description: "The most recent tasks in Hyalos's event log, newest first, with each task's status and the outcome of each of its jobs. The log is checked against the engine's public key first. Returns ids, statuses and times only.",
            input_schema: json!({
                "type": "object",
                "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": MAX_TASKS, "description": "How many tasks to return (default 20)."}},
                "additionalProperties": false,
            }),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "total": {"type": "integer"},
                    "tasks": {"type": "array", "items": {
                        "type": "object",
                        "properties": {
                            "task": {"type": "integer"},
                            "status": {"enum": ["open", "held", "completed", "cancelled"]},
                            "created_at_ms": {"type": "integer"},
                            "jobs": {"type": "array", "items": {
                                "type": "object",
                                "properties": {
                                    "job": {"type": "integer"},
                                    "status": {"enum": ["running", "succeeded", "failed", "deadline_exceeded", "held"]},
                                },
                                "required": ["job", "status"],
                            }},
                        },
                        "required": ["task", "status", "created_at_ms", "jobs"],
                    }},
                },
                "required": ["total", "tasks"],
            })),
            requires: &[],
        };
        Self {
            log,
            public_key,
            tools: vec![list_tasks],
        }
    }

    fn list_tasks(&self, arguments: &Map<String, Value>) -> Called {
        if arguments.keys().any(|k| k != "limit") {
            return Called::error("list_tasks takes only limit");
        }
        let limit = match arguments.get("limit") {
            None => 20,
            Some(n) => match n.as_u64().filter(|n| (1..=MAX_TASKS).contains(n)) {
                Some(n) => n,
                None => return Called::error("limit must be an integer from 1 to 100"),
            },
        };
        let key = match std::fs::read_to_string(&self.public_key) {
            Ok(text) => match PublicKey::from_hex(text.trim()) {
                Some(key) => key,
                None => {
                    return Called::error(
                        "the engine's public key is not 64 hexadecimal characters",
                    );
                }
            },
            Err(_) => return Called::error("there is no engine public key yet: run hyalos init"),
        };
        if !self.log.exists() {
            return Called::error("there is no event log yet: run hyalos init");
        }
        let state = match read_state(&self.log, &key) {
            Ok(state) => state,
            Err(e) => return Called::error(&format!("the event log does not check out: {e}")),
        };
        let tasks: Vec<Value> = state
            .tasks()
            .iter()
            .rev()
            .take(limit as usize)
            .map(|(id, task)| {
                let jobs: Vec<Value> = state
                    .jobs()
                    .iter()
                    .filter(|(_, job)| job.task == *id)
                    .map(|(job_id, job)| json!({"job": job_id.seq(), "status": job_status(job.ended.as_ref().map(|e| e.1))}))
                    .collect();
                json!({
                    "task": id.seq(),
                    "status": task_status(task.status()),
                    "created_at_ms": task.created_at.as_millis(),
                    "jobs": jobs,
                })
            })
            .collect();
        Called::structured(json!({"total": state.tasks().len(), "tasks": tasks}))
    }
}

impl Tools for EngineTools {
    fn tools(&self) -> &[Tool] {
        &self.tools
    }

    fn call(&self, name: &str, arguments: &Map<String, Value>, _: &mut Progress<'_>) -> Called {
        match name {
            "list_tasks" => self.list_tasks(arguments),
            _ => Called::error("unknown tool"),
        }
    }
}

fn task_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Open => "open",
        TaskStatus::Held => "held",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn job_status(outcome: Option<JobOutcome>) -> &'static str {
    match outcome {
        None => "running",
        Some(JobOutcome::Succeeded) => "succeeded",
        Some(JobOutcome::Failed) => "failed",
        Some(JobOutcome::DeadlineExceeded) => "deadline_exceeded",
        Some(JobOutcome::Held) => "held",
    }
}
