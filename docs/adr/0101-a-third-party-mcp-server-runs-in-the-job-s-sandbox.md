# ADR-0101: A third-party MCP server runs in the job's sandbox

**Status:** Accepted

## Context

[ADR-0006](0006-plugins-are-skills-and-wasm-components.md) says skill scripts and third-party MCP servers run in sandboxes behind the broker. S6.3 (#38) asks that a third-party MCP server run contained, with no network except what its manifest allows.

The pieces exist for skills:
- [ADR-0089](0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md) installs a skill by the operator's signed decision, pins its files by digest, and mounts them read-only in the job's sandbox.
- [ADR-0066](0066-plugin-authored-text-and-the-grant-that-bounds-it.md) cuts the job's manifest to what every skill it is given was granted.
- The sandbox has no network of its own ([ADR-0011](0011-pluggable-substrates.md)). Its only way out is the broker's egress gate, which allows the manifest's egress hosts and nothing else ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md)).

The operator weighed two places for a server to run:
- **The job's sandbox**, beside the harness, as a skill's scripts run. Nothing new crosses the trust boundary, and the job is cut to the server's grant.
- **A sandbox of its own**, with a manifest of its grant alone, and the broker relaying its stdio to the agent. This separates the server from the agent too. It needs a second sandbox per server and a relay on the channel, which [ADR-0086](0086-the-plugin-runtime-s-crate-and-line-budget.md)'s remaining lines would not cover.

The operator chose the job's sandbox.

## Decision

**An MCP server is a skill with an `mcp.json`.** Beside `SKILL.md`, `mcp.json` names a stdio server: `{"command": "...", "args": [...]}`, with no other field. The command is a program on the sandbox's `PATH` by its plain name, or `./path` to an executable file of the skill. An argument starting `./` must name a file of the skill. Every string is 1 to 512 bytes of printable ASCII, and there are at most 32 arguments. A skill whose `mcp.json` fails any of this does not read (`plugin.bad_mcp`), so it neither installs nor runs.

So a server installs as a skill does, through `hyalos plugin install` and the same `plugin.installed` event, with what it asks for in `capabilities.json` and the operator's grant. Its digest covers `mcp.json`, so a changed command refuses the run as any changed file does (`plugin.altered`). The install prints the command the server runs, which is printable ASCII and safe on the operator's terminal. No event, wire message or manifest field changes.

**A job given the skill registers the server with its harness.** `hyalos run --skill <name>` mounts the skill as before. The sandbox's command then names the server, from its read-only place at `/opt/hyalos/skills/<name>`:
- **Codex:** `-c mcp_servers.<name>.command`, `.args` and `.env`.
- **Claude Code:** `--mcp-config <json>`, and `mcp__<name>` added to `--allowedTools`, so its tools can be called in a run that asks nobody. Every Claude Code run is given `--strict-mcp-config`, with servers or without, so no server is loaded from a project file the actor finds in the repository it clones.

Each server is given the sandbox's proxy variables, since a harness need not pass its own environment on. A server that ignores them still has no other way out.

**Its network is the egress gate's.** The server runs in the job's sandbox, so it reaches only the hosts the job's manifest lists. That manifest is cut by the server's grant (ADR-0066), so the server reaches no host the operator did not grant it, whatever the job was started with.

## Consequences

S6.3's acceptance holds. A server granted one host, in a job started with that host and another, reaches the gate for both: the granted host is allowed, and the other is refused with `egress.host_not_allowed`. A server granted nothing leaves its job no egress at all.

The server is not separated from the agent. It runs as the sandbox's user, beside the harness, and holds nothing the agent could not already do, as a skill's scripts do (ADR-0089). A server that must keep something from the agent, or an agent that must be kept from a server, needs a sandbox of its own. That is a later decision, and a larger one.

Every server a job is given narrows the job. Giving a job two servers gives it only what both were granted.

The microVM substrate cannot carry skills, so it cannot carry servers either.

Only stdio servers are supported. A server reached over HTTP needs a host to reach, so it is a remote service that the egress gate already governs, not a contained one.

Costs: 114 lines of trusted core: reading `mcp.json` in `hyalos-plugin` (+54), and the harness command and the install line in `hyalos` (+60). Trusted core goes from 27,518 to 27,632 lines of 30,000.

Tests:
- `core/plugin/src/skill.rs`: `mcp.json` is read, and its paths placed under the skill's directory. A path outside the skill, a file it lacks, a command that is not executable, text that is not printable ASCII, too many arguments and an unknown field are refused.
- `core/hyalos/src/run_cmd/mod.rs`: both harnesses register the server with the proxy, and a skill that is not a server adds nothing.
- `core/hyalos/tests/run_image.rs` (needs the `hyalos-sandbox:test` image): Codex starts the server in the sandbox, and its requests reach the egress gate, which refuses the host the grant cut and only that one.
