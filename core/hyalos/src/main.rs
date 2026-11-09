//! The `hyalos` binary. The engine and broker run from this one executable (ADR-0002).

use std::process::ExitCode;

use hyalos::{
    agent_cmd, broker_cmd, doctor_cmd, github_cmd, hardening, init_cmd, log_cmd, mcp_cmd,
    merge_cmd, operator_cmd, plugin_cmd, policy_cmd, run_cmd, setup_cmd, stack, tools_cmd,
    workflow_cmd,
};

fn main() -> ExitCode {
    // Before anything reads a key: a crash must not be able to write it to disk (ADR-0030).
    if let Err(e) = hardening::no_core_dumps() {
        eprintln!("hyalos: {e}");
        return ExitCode::from(1);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    // On a thread with a stack of its own, so the policy engine can tell how much it has (#280).
    match stack::run(move || command(&args)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hyalos: cannot start: {e}");
            ExitCode::from(1)
        }
    }
}

fn command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        None | Some("--version") => {
            println!("hyalos {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("agent") => agent_cmd::run(args.get(1..).unwrap_or_default()),
        Some("init") => init_cmd::run(args.get(1..).unwrap_or_default()),
        Some("doctor") => doctor_cmd::run(args.get(1..).unwrap_or_default()),
        Some("github") => github_cmd::run(args.get(1..).unwrap_or_default()),
        Some("log") => log_cmd::run(args.get(1..).unwrap_or_default()),
        Some("mcp") => mcp_cmd::run(args.get(1..).unwrap_or_default()),
        Some("policy") => policy_cmd::run(args.get(1..).unwrap_or_default()),
        Some("operator") => operator_cmd::run(args.get(1..).unwrap_or_default()),
        Some("approve") => operator_cmd::approve(args.get(1..).unwrap_or_default()),
        Some("cancel") => operator_cmd::cancel(args.get(1..).unwrap_or_default()),
        Some("release") => operator_cmd::release(args.get(1..).unwrap_or_default()),
        Some("merge") => merge_cmd::run(args.get(1..).unwrap_or_default()),
        Some("run") => run_cmd::run(args.get(1..).unwrap_or_default()),
        Some("workflow") => workflow_cmd::run(args.get(1..).unwrap_or_default()),
        Some("plugin") => plugin_cmd::run(args.get(1..).unwrap_or_default()),
        Some("setup") => setup_cmd::run(args.get(1..).unwrap_or_default()),
        // The MCP server's tools, one command each (ADR-0075).
        Some(command @ ("tasks" | "start" | "status" | "result" | "approvals" | "stop")) => {
            tools_cmd::run(command, args.get(1..).unwrap_or_default())
        }
        // Started by the engine, never by hand; it refuses any other parent (ADR-0035).
        Some("broker") => broker_cmd::run(args.get(1..).unwrap_or_default()),
        Some(other) => {
            eprintln!(
                "hyalos: unknown command {other}\nusage: hyalos [--version | init | doctor [--image <image>] | setup [--next] [--json] | setup walk|operator-key|provider-key|github-app|policy|agent | github check <owner/repo> | run --repo <owner/name> --issue <text> ... | operator init|show | approve <hold> | cancel <task> | release <task> | merge <owner/repo> <pr> --head <commit> | policy check|approve|show ... | workflow check <file> | plugin install|list|diff ... | agent issue|revoke|list ... | mcp [--listen <127.0.0.1:port>] | tasks | start <owner/repo> <issue> | status <task> | result <task> | approvals | stop <task> | log verify|audit|backup|restore|compact|prune ...]"
            );
            ExitCode::from(2)
        }
    }
}
