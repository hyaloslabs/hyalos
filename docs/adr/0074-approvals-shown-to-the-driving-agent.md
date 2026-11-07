# ADR-0074: Approvals are shown to the driving agent as read-only tool results

**Status:** Proposed

## Context

[ADR-0010](0010-mcp-primary-interface-two-principals.md) makes MCP the primary interface and has approvals and results shown in the driving agent's client: "as MCP Apps where supported", as plain text otherwise. [ADR-0012](0012-humans-approve-only-irreversible-actions.md) and [ADR-0070](0070-the-operators-signature-on-every-human-decision.md) put every human decision on a channel of the operator's own, `hyalos approve` and `hyalos cancel`, signed with the operator's key behind their passphrase. The driving agent can therefore see an approval but must never be able to make it.

Before this, nothing showed the agent what was waiting. A held run read only "ended held (code)", and no tool listed the open holds or the command that decides them, so the agent could not tell the operator what to do.

Spike #92 found that neither target client renders MCP Apps (Claude Code 2.1.318, Hermes Agent 0.19.0). An MCP Apps view would also need the server to offer resources (`resources/list`, `resources/read`, a `ui://` resource), which ADR-0067 left out and the conformance suite's expected failures list as not offered.

## Decision

**A read-only tool, `list_approvals`, lists the open holds.** These are the holds not yet released whose task is not cancelled (`State::hold_is_open`), newest first, with an optional `limit` of 1 to 100 (default 20) and the total. Each one gives:
- `hold` and `task`;
- the `gate` and why it held (`unknown` or `timed_out`);
- `inputs`, the digest of what the gate was asked about;
- the stakes' `tier` and the sensitive categories it `touches`;
- `may_decide`, who may release it under the [ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md) table (`policy`, `verifier`, `operator`);
- `opened_at_ms`;
- the operator's commands, `hyalos approve <hold>` and `hyalos cancel <task>`.

**A held run shows its own holds.** Its `tasks/get` and `job_status` result carries the same details, and its status message names the command. Once none is open (decided by the operator, policy or the gate answering again), the view says nothing is waiting and names no command, since a held run does not continue on its own.

**Every field is a typed value from the log.** Ids, closed enums, a digest, a time and two commands Hyalos writes itself, so no text from the forge or a model is in an approval, and none is marked untrusted. Results that do carry such values stay marked as ADR-0072 says.

**The plain text is sentences, not JSON.** A client that reads only text gets the same details written out by Hyalos: what is waiting, that nothing in the server can decide it, and the commands to ask the operator to run. Structured content carries the same values.

**Nothing in the server decides.** The tools read the log and write nothing. No tool releases a hold, cancels a task or merges, and a test lists the server's tools exactly. S5.6 (#34) extends that check to every operator decision.

**No MCP Apps view yet.** Until a target client renders MCP Apps, the plain tool result is the view. Adding one later means offering resources, and that needs its own ADR and conformance run.

## Consequences

The driving agent can tell the operator exactly what is waiting and what to type, and still cannot decide anything. The operator reads the same facts the log holds. A future MCP Apps view would be a second rendering of these same values, not a new authority. Until S5.6, `list_approvals` has no CLI twin; that story adds it.
