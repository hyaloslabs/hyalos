# ADR-0073: Clients on older MCP revisions are unsupported until they move

**Status:** Proposed

## Context

[ADR-0067](0067-the-mcp-server-and-the-conformance-suite.md) has `hyalos mcp` serve only the stateless 2026-07-28 revision. Under "Revisions other than 2026-07-28", it says serving an older client "would need the stateful handshake back, which no target client needs".

Spike #92 found that premise is false for one target client. Hermes Agent 0.19.0, its latest release:
- pins the Python MCP SDK to 1.26.0, which speaks 2025-11-25 at most;
- opens every session with `initialize`, which `hyalos mcp` refuses (`-32601`, "this server speaks the stateless revision only").

The SDK's 2.3.0 release speaks 2026-07-28 and connects through `discover()`, so Hermes can move without anything changing in Hyalos. Claude Code 2.1.318 already speaks 2026-07-28 ([spike findings](../planning/spike-92-findings.md)).

Serving 2025-11-25 as well would bring back what ADR-0067 removed:
- sessions and the `initialize` handshake;
- a session id to bind the credential to, or the credential checked per session instead of per message;
- a second revision behind the protocol adapter, for one client, until that client updates.

## Decision

**Hyalos keeps serving only 2026-07-28.** A client on an older revision, Hermes Agent 0.19.0 included, is unsupported until it moves to 2026-07-28. Nothing is added to serve it. It is still told which revision this is, as ADR-0067 says.

**The requirement is stated where operators look.** `docs/operations/mcp.md` says that a client must speak 2026-07-28 (discovery and `_meta` on every request, not `initialize`), names the clients checked and their versions, and says that Hermes 0.19.0 does not speak it.

**This corrects only ADR-0067's premise.** ADR-0067's decision stands; its "no target client needs" sentence is wrong for Hermes, and this ADR records the choice made knowing that.

## Consequences

Out of the target clients, Claude Code drives Hyalos in M2 and Hermes does not, until it updates its SDK and calls `discover()`. The server keeps one revision, one credential check per message and no session state. When Hermes moves, it should work unchanged; re-check it with the spike rig (`spikes/92`) then. If a target client is stuck on an older revision for long, revisit this with a new ADR.
