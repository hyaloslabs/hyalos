//! The driving agent's MCP tools on the command line, one command each (S5.6, ADR-0075): `hyalos
//! tasks`, `start`, `status`, `result` and `approvals`. Each builds the server's own `EngineTools`
//! and makes the same call, so a command and its tool cannot answer differently.
//!
//! Operator decisions are not here, and no MCP tool makes one: approving, cancelling, merging,
//! approving policy, issuing or revoking a credential and releasing an issue are commands of their
//! own, behind the operator's passphrase (ADR-0012, ADR-0070). [`COMMANDS`] and [`DECISIONS`] are
//! what the tests hold the server's tools to.

use std::process::ExitCode;

use serde_json::{Map, Value, json};

use crate::mcp::{Called, Progress, Tools};
use crate::mcp_cmd::{self, EngineTools};
use crate::run_cmd::Request;

/// Each command and the MCP tool it calls, in the server's order. Every tool has exactly one.
pub const COMMANDS: [(&str, &str); 5] = [
    ("tasks", "list_tasks"),
    ("start", "start_job"),
    ("status", "job_status"),
    ("result", "job_result"),
    ("approvals", "list_approvals"),
];

/// The operator's decisions, as commands. None of them is an MCP tool, or ever may be.
pub const DECISIONS: [&str; 7] = [
    "approve",
    "cancel",
    "merge",
    "policy approve",
    "agent issue",
    "agent revoke",
    "run --release-issue",
];

const USAGE: &str = "usage: hyalos tasks [--limit <n>] [--json]
       hyalos start <owner/repo> <issue-number> [--base <branch>] [--harness codex|claude-code]
                    [--image <image>] [--budget-tokens <n>] [--minutes <n>] [--step-model <model>] [--json]
       hyalos status <task> [--json]
       hyalos result <task> [--json]
       hyalos approvals [--limit <n>] [--json]
  the MCP server's tools, one command each (list_tasks, start_job, job_status, job_result,
  list_approvals): the same call and the same answer. Each prints the tool's text, or with --json its
  structured content. start starts hyalos run as a process of its own, as start_job does, with these
  options (as for hyalos mcp). Exit 1 when the tool answers with an error, 2 for bad usage.
  None of them decides anything: hyalos approve, cancel and merge are commands of their own";

/// Runs `command` (one of [`COMMANDS`]) with `args`.
pub fn run(command: &str, args: &[String]) -> ExitCode {
    let (json, args): (Vec<_>, Vec<_>) = args.iter().partition(|a| *a == "--json");
    let (tool, arguments, template) = match parse(command, &args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("hyalos: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let env = |name: &str| std::env::var(name).ok();
    let (log, launcher) = match mcp_cmd::setup(&env) {
        Ok(found) => found,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    let tools = EngineTools::new(log, launcher, template);
    let mut ignore = |_: Value| {};
    let called = tools.call(tool, &arguments, &mut Progress::untracked(&mut ignore));
    println!("{}", shown(&called, !json.is_empty()));
    if called.is_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// The tool `command` calls, its arguments, and the template `start` runs with.
fn parse(
    command: &str,
    args: &[&String],
) -> Result<(&'static str, Map<String, Value>, Request), String> {
    let tool = COMMANDS
        .iter()
        .find(|(c, _)| *c == command)
        .map(|(_, t)| *t)
        .ok_or_else(|| format!("unknown command {command}"))?;
    let mut arguments = Map::new();
    let mut template_args = Vec::new();
    match tool {
        "list_tasks" | "list_approvals" => match args {
            [] => {}
            [flag, n] if flag.as_str() == "--limit" => {
                let n: u64 = n.parse().map_err(|_| "--limit is a number")?;
                arguments.insert("limit".into(), json!(n));
            }
            _ => return Err(format!("{command} takes only --limit <n>")),
        },
        "job_status" | "job_result" => match args {
            [task] => {
                arguments.insert("task".into(), json!(task));
            }
            _ => return Err(format!("{command} takes one task")),
        },
        _ => match args {
            [repo, number, rest @ ..] => {
                let number: u64 = number
                    .parse()
                    .map_err(|_| "the issue number is a positive integer")?;
                arguments.insert("repo".into(), json!(repo));
                arguments.insert("issue_number".into(), json!(number));
                template_args = rest.iter().map(|s| (*s).clone()).collect();
            }
            _ => return Err("start takes <owner/repo> <issue-number>".into()),
        },
    }
    let (listen, template) = mcp_cmd::options(&template_args)?;
    if listen.is_some() {
        return Err("--listen is for hyalos mcp".into());
    }
    Ok((tool, arguments, template))
}

/// What a call printed: its text, or with `json` its structured content (its text if it has none).
fn shown(called: &Called, json: bool) -> String {
    if json && let Some(value) = &called.structured {
        return serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    }
    called
        .content
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn parsed(command: &str, list: &[&str]) -> Result<(&'static str, Map<String, Value>), String> {
        let owned = args(list);
        let refs: Vec<&String> = owned.iter().collect();
        parse(command, &refs).map(|(tool, arguments, _)| (tool, arguments))
    }

    #[test]
    fn each_command_calls_its_tool_with_the_tools_own_arguments() {
        assert_eq!(parsed("tasks", &[]), Ok(("list_tasks", Map::new())));
        let (tool, a) = parsed("approvals", &["--limit", "5"]).unwrap();
        assert_eq!(
            (tool, Value::Object(a)),
            ("list_approvals", json!({"limit": 5}))
        );
        let (tool, a) = parsed("status", &["12"]).unwrap();
        assert_eq!(
            (tool, Value::Object(a)),
            ("job_status", json!({"task": "12"}))
        );
        let (tool, a) = parsed("result", &["12"]).unwrap();
        assert_eq!(
            (tool, Value::Object(a)),
            ("job_result", json!({"task": "12"}))
        );
        let (tool, a) = parsed("start", &["octo/repo", "7", "--base", "dev"]).unwrap();
        assert_eq!(
            (tool, Value::Object(a)),
            ("start_job", json!({"repo": "octo/repo", "issue_number": 7}))
        );
    }

    #[test]
    fn bad_usage_is_refused_before_anything_is_called() {
        for (command, list) in [
            ("tasks", &["--limit"][..]),
            ("tasks", &["--limit", "x"]),
            ("tasks", &["7"]),
            ("status", &[]),
            ("status", &["1", "2"]),
            ("start", &["octo/repo"]),
            ("start", &["octo/repo", "seven"]),
            ("start", &["octo/repo", "7", "--listen", "127.0.0.1:1"]),
            ("start", &["octo/repo", "7", "--issue", "do it"]),
            ("approve", &["3"]),
        ] {
            assert!(parsed(command, list).is_err(), "{command} {list:?}");
        }
    }

    #[test]
    fn no_command_here_is_an_operator_decision() {
        for (command, tool) in COMMANDS {
            assert!(
                DECISIONS
                    .iter()
                    .all(|d| d.split(' ').next() != Some(command)),
                "{command} is a decision"
            );
            for word in ["approve", "cancel", "merge", "release", "revoke", "policy"] {
                assert!(!tool.contains(word), "{tool}");
            }
        }
    }
}
