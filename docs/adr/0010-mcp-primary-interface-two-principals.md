# ADR-0010: MCP is the primary interface; two principals

**Status:** Accepted on November 1, 2026, after review against ADR-0002, 0004, 0005 and 0012 and against the MCP 2026-07-28 changelog. The clarifications from that review are folded in below.

## Context

Hyalos is meant to be driven by an agent. The MCP 2026-07-28 revision made the core protocol stateless. Tasks (long-running jobs with polling) and MCP Apps (interactive UI inline in the client) are official extensions of that revision, not core. Extensions are off by default and negotiated per request; a server must serve a client that does not advertise one. Which of our target clients support them is not established.

Sources, read November 1, 2026: [2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog) and the [extensions overview](https://modelcontextprotocol.io/extensions). The overview lists MCP Apps as Stable and MCP Tasks as "Beta, to Core", and states no client support for MCP Apps.

## Decision

The engine's primary API is an MCP server on the 2026-07-28 revision. The CLI and HTTP API are thin wrappers. By default the server listens on a local socket only, with no network listener unless the operator enables one ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)).

**Jobs are MCP Tasks where the client supports the extension.** Otherwise the same start, status and result operations are offered as plain tools, with the same semantics.

**Approvals and results render as MCP Apps where the client supports the extension, and as plain text otherwise.** An approval view is display only. It shows the request and the operator command; it never resolves anything. An approval resolves only through the operator channel ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)): in v1 `hyalos approve <id>` with a local signing key, passkeys later.

**Two principals.** The **driving agent** authenticates with a credential the operator issues and can revoke. It can start work and read results. It can never approve its own escalations and cannot sign approvals. The **operator** approves through a separate channel the driving agent cannot use.

**Results carry provenance.** Text derived from tainted input is returned marked as untrusted data, and the driving agent never receives credentials or manifest signing material ([ADR-0005](0005-provenance-is-a-type.md)).

## Consequences

No dashboard to build. Hyalos works from any MCP client, with a plainer experience where the Tasks and Apps extensions are missing; the guarantees do not change, because approval never depended on the client's UI. Protocol code sits behind an adapter because MCP revises yearly. Marking tainted text helps a driving agent treat it as data but cannot force it to, so this narrows the path from attacker-written text to the front door without closing it.
