# Spike #92: which target clients support the Tasks and MCP Apps extensions

Working material, run November 7, 2026. It answers the question [ADR-0010](../adr/0010-mcp-primary-interface-two-principals.md) left open: which of the PRD's target clients support the Tasks and MCP Apps extensions. The evidence and the method are kept here; the rig is in [`spikes/92`](../../spikes/92/README.md).

## Question

For Claude Code and Hermes:
- Do they support the Tasks extension?
- Do they support MCP Apps?
- Can a client with neither run a job through Hyalos's plain tools with the same guarantees?

## Method

- **Probe server** (`spikes/92/probe.py`): an MCP server on stdio that logs every message a client sends. It answers both the 2025-11-25 handshake (`initialize`) and the 2026-07-28 stateless revision (`server/discover`, `_meta` on every request). It offers one tool that declares `taskSupport: optional` and an MCP Apps UI resource (`ui://probe/view.html`). A client that supports either extension would therefore show it in what it sends.
- **Claude Code:** one `claude -p` run with a temporary `--mcp-config` and `--strict-mcp-config`, asked to call the probe's tool once. A second run against a real `hyalos mcp` in a throwaway world (`hyalos init`, `operator init`, `agent issue`) called `list_tasks` and `job_status`.
- **Hermes Agent:** its package, read but not run. Its MCP client is `tools/mcp_tool.py`, which uses the official Python MCP SDK's `stdio_client` and `ClientSession` and calls `session.initialize()`. That client was then reproduced exactly with the same SDK version against `hyalos mcp` (`spikes/92/sdk_initialize.py`), and with the newest SDK's `discover()` (`spikes/92/sdk_discover.py`).
- **Versions:** Claude Code 2.1.318; Hermes Agent 0.19.0 (its latest), which pins `mcp==1.26.0`; Python MCP SDK 2.3.0 (its latest); `hyalos` built from `537371c` and `5116378` (S5.10, PR #237, on top of `888996a`). The behaviour checked, refusing `initialize` and offering only 2026-07-28 on discovery, is the same on `888996a`.

## Results

| Check | Claude Code 2.1.318 | Hermes Agent 0.19.0 |
| --- | --- | --- |
| MCP revision it speaks | 2026-07-28 (`server/discover`, then `_meta` on every request) | 2025-11-25 at most (SDK 1.26.0; `initialize` handshake) |
| Connects to `hyalos mcp` | Yes: `list_tasks` and `job_status` answered | **No**: `initialize` is refused (`-32601`, "this server speaks the stateless revision only")¹ |
| Tasks extension | Declares `io.modelcontextprotocol/tasks` on discovery and listing, but not on `tools/call`, and sends no task parameter. In practice every call is a plain call. | No extension. The SDK offers the 2025-11-25 core tasks capability, but Hermes never calls a tool as a task. |
| MCP Apps | No: does not declare `io.modelcontextprotocol/ui` and never reads the UI resource | No: no MCP Apps code at all |
| Reads `structuredContent` | Yes | Yes: content and structured content are combined |

¹ Hermes itself was not run. Its MCP client was reproduced with its pinned SDK (`spikes/92/sdk_initialize.py`).

The Python SDK 2.3.0 supports 2026-07-28, but `initialize()` is still the old handshake there and is refused. Its `discover()` connects to `hyalos mcp` and lists all four tools. Hermes would need both the newer SDK and a change to call `discover()`.

## What it means

- **Neither client renders MCP Apps.** Approvals and results show as plain text in both (S5.3, #31). Read-only views stay the right design, since no client would show anything else.
- **Neither client runs Hyalos's jobs as MCP tasks.** Claude Code sends every `tools/call` with no Tasks extension and no task parameter, so it would get `start_job`'s plain answer and use `job_status` and `job_result`: the plain-tool fallback ADR-0010 provides.
- **What was seen, and what was not.** Against a real `hyalos mcp`, Claude Code was seen to call `list_tasks` and `job_status` and read their answers. The world was fresh, so those were an empty list and a "there is no such task" tool error: they show the plain call path and the credential working, not a task being read. No client called `start_job` or `job_result` here, and no job was completed, because that needs a GitHub App and a provider. What the plain path guarantees is shown by tests, not by a client:
  - `start_job` takes only identifiers;
  - `job_status` and `job_result` read the same task as `tasks/get` (`core/hyalos/tests/mcp.rs`, `a_runs_task_is_polled_with_the_tasks_extension_or_its_plain_tools_and_reads_the_same`, which writes the run's events itself);
  - with PR #237 (S5.10), the result is marked untrusted in the text as well.

  A client completing a job through the plain tools against a real forge is left to S5.7 (#74), the full reference job through MCP from Claude Code.
- **Nothing here shows the fallback unworkable, so ADR-0010 stands.** No ADR supersedes it.
- **Hermes is blocked one layer below the extensions,** by the protocol revision ADR-0067 chose. Either Hyalos also serves 2025-11-25, which would supersede part of ADR-0067, or Hermes is documented as unsupported until it moves to 2026-07-28. That is a decision for its own story, S5.12 (#239).

## Proposed follow-up

S5.12 (#239), under E5 (#28): "As an operator, I drive Hyalos from a client on the 2025-11-25 MCP revision, such as Hermes". ADR-0067 rests on the stateful handshake being something "no target client needs", and Hermes shows that premise is false. The story's ADR must either supersede that part of ADR-0067 or say why it stands. It would:
- choose, in an ADR, between serving 2025-11-25 too (behind the same adapter, credential and guarantees) and documenting the revision a client needs;
- then either show Hermes completing `list_tasks`, `start_job`, `job_status` and `job_result`, or say in `docs/operations/mcp.md` that Hermes 0.19.0 cannot connect.

Re-check these results when either client changes version.

