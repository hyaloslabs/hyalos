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

/// The Tasks extension's methods that act on a run, one command each. `stop` is `tasks/cancel`: it
/// asks a run the server started to stop, as the agent can. It is not `hyalos cancel`, the operator's
/// signed decision on a held task.
pub const TASK_METHODS: [(&str, &str); 1] = [("stop", "tasks/cancel")];

/// The operator's decisions, as commands. None of them is an MCP tool, or ever may be.
pub const DECISIONS: [&str; 13] = [
    "approve",
    "cancel",
    "release",
    "merge",
    "policy approve",
    "agent issue",
    "agent revoke",
    "plugin install",
    "plugin policy set",
    "run --release-issue",
    "setup walk",
    "setup policy",
    "setup agent",
];

const USAGE: &str = "usage: hyalos tasks [--limit <n>] [--json]
       hyalos start <owner/repo> <issue-number> [--base <branch>] [--harness codex|claude-code]
                    [--image <image>] [--budget-tokens <n>] [--minutes <n>] [--step-model <model>] [--json]
       hyalos status <task> [--json]
       hyalos result <task> [--json]
       hyalos approvals [--limit <n>] [--json]
       hyalos stop <task>
  the MCP server's tools, one command each (list_tasks, start_job, job_status, job_result,
  list_approvals): the same call and the same answer. Each prints the tool's text, or with --json its
  structured content. start starts hyalos run as a process of its own, as start_job does, with these
  options (as for hyalos mcp). status and result, like the server, resume a run whose runner
  stopped, and that run writes to the log. stop asks a run start started to stop, as tasks/cancel
  does. Exit 1 when the tool answers with an error, 2 for bad usage.
  None of them decides anything: hyalos approve, cancel and merge are commands of their own";

/// Options whose next argument is their value, so a `--json` there is a value, not the flag.
const VALUED: [&str; 8] = [
    "--limit",
    "--base",
    "--harness",
    "--image",
    "--budget-tokens",
    "--minutes",
    "--step-model",
    "--listen",
];

/// Whether `--json` was given as a flag, and the other arguments.
fn split_json(args: &[String]) -> (bool, Vec<&String>) {
    let mut json = false;
    let mut rest = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let is_value = i > 0 && VALUED.contains(&args[i - 1].as_str());
        if arg == "--json" && !is_value {
            json = true;
        } else {
            rest.push(arg);
        }
    }
    (json, rest)
}

/// Runs `command` (one of [`COMMANDS`] or [`TASK_METHODS`]) with `args`.
pub fn run(command: &str, args: &[String]) -> ExitCode {
    if command == "stop" {
        return stop(args);
    }
    let (json, args) = split_json(args);
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
    println!("{}", shown(&called, json));
    if called.is_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `hyalos stop <task>`: the server's `tasks/cancel`, which answers an unknown task the same way.
fn stop(args: &[String]) -> ExitCode {
    let [task] = args else {
        eprintln!("hyalos: stop takes one task\n{USAGE}");
        return ExitCode::from(2);
    };
    let env = |name: &str| std::env::var(name).ok();
    // Cancelling starts nothing, so the template is the server's with no options.
    let found = mcp_cmd::setup(&env).and_then(|(log, launcher)| {
        let (_, template) = mcp_cmd::options(&[])?;
        Ok(EngineTools::new(log, launcher, template))
    });
    let tools = match found {
        Ok(tools) => tools,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    if tools.cancel(task) {
        println!("asked task {task} to stop");
        ExitCode::SUCCESS
    } else {
        println!("there is no such task started by hyalos start or start_job");
        ExitCode::from(1)
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

/// What a call printed: its text, or with `json` its structured content (or, for an answer with
/// none, its content and whether it is an error), so `--json` is always JSON.
fn shown(called: &Called, json: bool) -> String {
    if json {
        let value = called
            .structured
            .clone()
            .unwrap_or_else(|| json!({"isError": called.is_error, "content": called.content}));
        return serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
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
    fn json_is_the_flag_only_where_it_is_not_an_options_value() {
        let given = args(&["octo/repo", "7", "--step-model", "--json", "--json"]);
        let (json, rest) = split_json(&given);
        assert!(json);
        assert_eq!(rest, ["octo/repo", "7", "--step-model", "--json"]);
        let given = args(&["--json", "12"]);
        assert_eq!(split_json(&given), (true, vec![&given[1]]));
    }

    #[test]
    fn no_command_here_is_an_operator_decision() {
        // No command shares a decision's name. `stop` (tasks/cancel) only stops a run the agent
        // started, which is not a decision, and is not `hyalos cancel`.
        for (command, _) in COMMANDS.into_iter().chain(TASK_METHODS) {
            assert!(
                DECISIONS
                    .iter()
                    .all(|d| d.split(' ').next() != Some(command)),
                "{command} is a decision"
            );
        }
        // No tool is named for a decision.
        for (_, tool) in COMMANDS {
            for word in [
                "approve", "cancel", "merge", "release", "revoke", "policy", "install", "grant",
            ] {
                assert!(!tool.contains(word), "{tool}");
            }
        }
    }
}
