# ADR-0003: The event log is the database

**Status:** Accepted on November 1, 2026, after review against ADR-0002, 0004, 0005, 0009, 0010 and 0012. The clarifications from that review are folded in below.

## Context

Today's hivrr keeps tasks, audit chains, checkpoints and config in separate stores, and bugs live in the seams.

## Decision

Every state change is an append-only, hash-chained, signed event in SQLite (WAL). Tasks, jobs, holds and config are projections. Schemas are versioned and old events are upcast on read.

**The engine is the only writer.** Only the engine process opens the database; sandboxes and the broker never have access to it. The broker holds no signing key ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)), so the events it produces (refused egress, metering, content-gate results, unknown gate answers) are submitted to the engine over the private channel. The engine appends and signs them, and each event records its origin: engine or broker.

**Events hold typed values, digests and references.** They never contain credentials, forge tokens, provider keys, manifest signing material, raw model text or raw forge content ([ADR-0002](0002-one-binary-two-processes-sandboxes.md), [ADR-0005](0005-provenance-is-a-type.md)). Raw text that a gated handle must later release is held by the broker, not the engine, and referenced from the log by digest. The engine never holds raw model text or raw forge content. The driving agent reads results and learnings only through the MCP surface ([ADR-0010](0010-mcp-primary-interface-two-principals.md)), never the database.

**Approvals carry the approver's own signature.** A human approval event includes the operator's signature from their local key ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)), alongside the engine's, so the log shows who approved and the engine cannot fabricate a human approval. A verifier agent's verdict is recorded as such.

**Operator policy is config like any other.** A policy change enters the log only as an approved event holding the policy document, and the policy in force is the projection. Default policies shipped with Hyalos live in the repository.

**Compaction does not destroy the audit trail.** A signed snapshot anchors the hash chain so `hyalos log verify` still works across it. Compacted segments are archived and can be exported. Pruning an archive is an explicit operator action, taken through the operator channel and never by the driving agent.

## Consequences

Audit, replay, debugging, backup (snapshot plus replay) and learnings share one mechanism. Deterministic simulation becomes possible. Projections must be rebuildable, and log growth needs compaction by snapshot. Because the broker's events pass through the engine, a compromised broker can report false events or omit them, so the log records what the broker says, not independent proof of it. Its refusals still hold, because enforcement happens in the broker before anything is logged.
