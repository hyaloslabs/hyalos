# ADR-0010: MCP is the primary interface; two principals

**Status:** Proposed

## Context

Hyalos is meant to be driven by an agent. The MCP 2026-07-28 revision made the core protocol stateless and added Tasks and MCP Apps.

## Decision

The engine's primary API is an MCP server on the 2026-07-28 revision. Jobs are MCP Tasks; approvals and results render as MCP Apps in the driving client. The CLI and HTTP API are thin wrappers. There are two principals: the **driving agent**, which can start work and read results, and the **operator**, who approves through a separate channel (local signing key or passkey). The driving agent can never approve its own escalations.

## Consequences

No dashboard to build. Hyalos works from any MCP client. Protocol code sits behind an adapter because MCP revises yearly.
