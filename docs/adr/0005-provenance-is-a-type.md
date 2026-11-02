# ADR-0005: Provenance is a type; step roles are checked at install

**Status:** Proposed

## Context

The reader/planner split, taint gates and `no_raw_text` were retrofitted across three PRDs and checked at runtime.

## Decision

Every value carries its provenance (operator, forge-authored, model output derived from tainted input). Workflow steps declare a role: **reader** (no tools, emits typed values), **planner** (typed values only), **actor** (typed inputs, raw text only through a gated handle) or **deterministic**. The engine type-checks a workflow at install; letting tainted text reach an actor's prompt is an install error.

**Where roles run.** A reader or planner without tools needs no sandbox: a reader is a schema-constrained model call made and validated in the broker ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)), and the engine receives only the typed values. A planner that needs read access to the code runs in a sandbox with read-only tools, as an actor variant ([ADR-0016](0016-agent-runtime-and-broker-protocols.md)).

## Consequences

Layer 3 holds by construction. Workflow authors must define typed schemas for what a reader extracts. The broker still enforces roles at runtime (a reader's key refuses tools) as a second wall.
