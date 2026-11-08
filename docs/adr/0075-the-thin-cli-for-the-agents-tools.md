# ADR-0075: The thin CLI: one command for each of the driving agent's tools

**Status:** Proposed

## Context

[ADR-0010](0010-mcp-primary-interface-two-principals.md) makes MCP the primary interface and promises a thin CLI for the same actions, so that neither can do something the other cannot. [ADR-0012](0012-humans-approve-only-irreversible-actions.md) and [ADR-0070](0070-the-operators-signature-on-every-human-decision.md) keep every operator decision off the driving agent's channel. Such a decision is signed with the operator's key behind their passphrase, at a command of its own.

The server offers five tools: `list_tasks`, `start_job`, `job_status`, `job_result` ([ADR-0069](0069-jobs-the-driving-agent-starts-as-mcp-tasks.md)) and `list_approvals` ([ADR-0074](0074-approvals-shown-to-the-driving-agent.md)). Only `hyalos run` was close to one of them, and it is a different thing: a runner in the foreground with the operator's own options and text. Nothing kept the two surfaces equal, and nothing stopped a deciding tool from being added to the server.

## Decision

**Each tool has one command, which makes the same call.** The commands are:

| Command | Tool |
| --- | --- |
| `hyalos tasks [--limit <n>]` | `list_tasks` |
| `hyalos start <owner/repo> <issue-number> [<template options>]` | `start_job` |
| `hyalos status <task>` | `job_status` |
| `hyalos result <task>` | `job_result` |
| `hyalos approvals [--limit <n>]` | `list_approvals` |
| `hyalos stop <task>` | `tasks/cancel` (the Tasks extension) |

`stop` is the one thing the agent does to a run outside a tool: it asks a run the server started to stop, as `tasks/cancel` does (`EngineTools::cancel`, the run's `task-<id>.cancel`). It is not `hyalos cancel`, the operator's signed decision on a held task ([ADR-0029](0029-cancelling-a-held-task-and-recording-a-refusal.md)), and it shares no name with one.

Each one builds the server's own `EngineTools` and calls `Tools::call` with the tool's arguments, so the checks, the answer and its text are the same code. `start` takes the same template options as `hyalos mcp` (`--base`, `--harness`, `--image`, `--budget-tokens`, `--minutes`, `--step-model`), and nothing the agent could not give, so it starts a runner exactly as `start_job` does.

**The output is the tool's.** A command prints the tool's text, or with `--json` its structured content (for an answer with none, its content and whether it is an error, so `--json` is always JSON). It exits 1 when the tool answers with an error, and 2 for bad usage, which is refused before anything is called.

**They act as the server acts.** `start` starts a runner. `status` and `result`, like `tasks/get`, resume a run whose runner stopped ([ADR-0069](0069-jobs-the-driving-agent-starts-as-mcp-tasks.md)), and the resumed run writes to the log with the engine's key. None of them decides anything.

**No credential.** The agent's credential ([ADR-0068](0068-the-driving-agents-credential.md)) admits callers on the MCP channel. These commands run as the operator's user on the operator's machine, as `hyalos run` and `hyalos log verify` already do. An agent with a shell as that user is already past the credential's boundary, since it could run `hyalos run` itself, so these commands add nothing to it. Revoking a credential closes the MCP channel, not a shell.

**Operator decisions stay CLI-only, and a test keeps them so.** `tools_cmd::COMMANDS` maps each command to its tool, and `tools_cmd::DECISIONS` lists the decisions:
- `approve`, `cancel`, `merge`;
- `policy approve`;
- `agent issue`, `agent revoke`;
- `run --release-issue`.

`tools_cmd::TASK_METHODS` maps `stop` to `tasks/cancel`. `tests/mcp.rs` requires the server's tool list to be exactly the tools in `COMMANDS`, so a tool added without its command fails, and checks that each command answers as its tool does and that `stop` asks what `tasks/cancel` asks. A unit test requires that no command shares a decision's name, and that no tool's name approves, cancels, merges, releases, revokes or touches policy.

**Setup and maintenance stay operator tools with no MCP tool:** `init`, `doctor`, `operator`, `github check`, `log ...` and `workflow check`. They set up or inspect the operator's own machine, and none of them is something the driving agent asks for.

## Consequences

The CLI and the MCP server answer alike by construction, so a change to one tool is a change to its command. A new tool needs a command in the same change, and a deciding tool cannot be added without a test saying no. `hyalos run` stays the operator's own foreground runner, alongside `hyalos start`.
