# ADR-0068: The driving agent's credential, issued and revoked in the log

**Status:** Proposed

## Context

S5.8 (#76) closes gap G1 of the reference-job walk: how the driving agent authenticates to the engine's MCP server, and how the operator issues that credential. [ADR-0010](0010-mcp-primary-interface-two-principals.md) names two principals:
- **The driving agent** authenticates with a credential the operator issues and can revoke. It may start work and read results, and it may never sign approvals.
- **The operator** approves through a separate channel.

The story's acceptance is:
1. The driving agent reaches the engine's MCP server only with a credential the operator issued.
2. That credential cannot sign approvals (S5.4).
3. The operator can revoke it, and the agent's next call fails.

[ADR-0067](0067-the-mcp-server-and-the-conformance-suite.md) shipped the server with no authentication. That was safe only because its one tool reads ids and statuses. S5.2 adds tools that start jobs, so the server needs a credential in front of it before then.

What shapes the decision:
- **The server keeps nothing between calls**, and it reads the log with the engine's public key alone (ADR-0067). It must not open `engine.key`, because the driving agent's server process should hold nothing that writes or signs.
- **MCP 2026-07-28 makes authorization optional.** Its OAuth flow is for HTTP servers that a remote authorization server fronts. This server is on loopback or on stdio, launched by the client. On stdio the revision says a server takes its credentials from its environment.
- **Revocation must take effect at the next call.** A credential cached at start could outlive its revocation.

## Decision

**A credential is a random bearer secret, and the log records only its SHA-256.**
- `hyalos agent issue <name>` draws 32 bytes from the operating system and prints `hyalos-agent-` and their hex on stdout, once, with how to use it on stderr. The prefix makes a leaked credential recognisable.
- It writes `agent.issued { name, credential, by, reason }`, where `credential` is the SHA-256 of the full text and `name` is an `AgentName`: 1 to 64 lowercase letters, digits, `.`, `_` and `-`, so it carries no free text.
- The new agent's id (`AgentId`) is the event's sequence number, as for every other id.
- Neither the log nor a backup of it holds the secret, and nothing else stores it. A lost credential is revoked and another issued.

**Issuing needs a human; revoking needs any decision.**
- Issuing grants a capability. Under the [ADR-0025](0025-who-may-release-a-hold.md) table, an irreversible action touching a capability grant needs a human. The state refuses any other decider (`AgentNeedsAHuman`), as it does for a policy change or a handle's release. As there, the human claim is the caller's until operator signatures (S5.4).
- Revoking only takes a capability away, so a human, policy or verifier may do it. A gate's re-evaluation is not a decision and is refused (`RevokeNotADecision`). Revoking an unknown or already revoked agent is refused.
- A credential is never issued twice, revoked or not (`AgentCredentialReused`), so a revoked one cannot come back. A name belongs to at most one credential in force (`AgentNameInUse`); once that one is revoked, the name may be issued again.
- Both events are engine-only. They are written only by `Engine::issue_agent` and `Engine::revoke_agent`: `submit` refuses them raw, and the broker may not send them.
- This is schema version 11, which adds the two kinds and changes no payload. `StateImage` version 6 keeps every agent, revoked ones too, as `AgentImage`, so compaction keeps them. An older image reads as having none.

**The server admits a caller on every message, from the log.**
- `hyalos::mcp::Server` now takes an `Admit`. `Server::handle(message, headers, credential, emit)` puts the presented credential to it before reading anything else in the message, even before checking that the message is JSON-RPC.
- A refusal is HTTP 401 with JSON-RPC error `-32030`. That code is Hyalos's own, from the range JSON-RPC leaves to servers. The answer says the same thing for a missing credential, an unknown one and a revoked one.
- If the credentials cannot be checked, the answer is 503 with `-32603` and no one is admitted. The cause, such as a missing log, goes to the operator on stderr, never to the caller.
- The engine's `Admit` (`mcp_cmd::LogAdmission`) reads the log with `engine.pub` on each message, hashes the credential, and admits it only if `State::admitted_agent` finds an agent with that digest that is not revoked. So a revocation written by `hyalos agent revoke` fails the very next call, on a server that is already running.
- The core holds no admission that lets everyone in. The conformance crate defines its own for its fixtures, since the suite presents no credential.

**Where each transport takes the credential:**
- **stdio:** the server's `HYALOS_AGENT_CREDENTIAL`, as MCP prescribes for stdio. It is checked once at start, so a wrong setup refuses to start and says why (exit 1), then again on every message.
- **HTTP:** each request's `Authorization: Bearer <credential>`. The scheme is case-insensitive. Any other scheme, or none, is no credential and gets 401 with `WWW-Authenticate: Bearer realm="hyalos"`. Host, Origin, path and method are still checked first, so a rebinding page is refused before its credential is considered.

**The credential cannot sign approvals.** It is a bearer secret, not a key: it signs nothing, and the engine accepts no approval on the strength of one.
- Issuing, revoking and every approval in v1 (`policy approve`, the handle release in `hyalos run`, hold decisions) are written with `engine.key` by an operator command, never by the MCP server, which never opens that key.
- The server's only tool reads.
- S5.4 adds `hyalos approve` signed with an operator key. It must refuse anything that carries only this credential, and its test checks that from the approval side.

**`hyalos agent list`** reads the log with the public key alone. It shows each agent's id, name, when it was issued, and whether and why it was revoked, never the credential or its digest.

## Consequences

The server has a front door. A process that can start `hyalos mcp`, or reach its loopback port, gets nothing without a credential the operator issued, and revoking one takes effect at its next call with no restart. The log shows who was given access and when it was taken away, and an audit of the log reveals no secret.

**What this does not protect against.** The driving agent runs as the operator's user. A process with that user's file access can read `engine.key`, and so could write events. Keeping the agent from the operator's files is a matter of how the agent itself is run, and of the separate operator key (S5.4, passkeys later). This credential decides what the MCP server answers, nothing more.

**Cost.** Each message reads and verifies the log twice, once to admit and once in `list_tasks`. Both checks are linear in the events since the latest snapshot. That is acceptable at this server's call rates and log sizes, and compaction bounds it. A cache keyed by the log's head is possible later behind the same `Admit`.

**Tests:**
- `core/engine/tests/agents.rs`: authority, uniqueness, raw events refused, replay and compaction, and a read with the public key alone.
- `core/hyalos/src/mcp/mod.rs`: refusal before anything is read, 401 and 503 with no detail, stdio, bearer parsing.
- `core/hyalos/tests/mcp.rs`: the binary on stdio and HTTP, a revocation while the HTTP server runs, and the log scanned for the secret.

**Costs.** 564 more lines of trusted core:
- the state, the engine calls and the types (types +92, engine +225);
- `hyalos agent` and the admission (hyalos +247, of which 52 are in `core/hyalos/src/mcp`).

Trusted core goes from 17,337 to 17,901 lines of 30,000.
