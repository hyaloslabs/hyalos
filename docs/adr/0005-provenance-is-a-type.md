# ADR-0005: Provenance is a type; step roles are checked at install

**Status:** Proposed. Decisions on harness actors and plugin-authored text were made in the November 1, 2026 planning session; folded in below.

## Context

The reader/planner split, taint gates and `no_raw_text` were retrofitted across three PRDs and checked at runtime.

## Decision

Every value carries its provenance: **operator**, **forge-authored** (issue text, comments, and the contents of a repository, since contributors write them), **plugin-authored** (instructions and output from an installed skill or plugin), or **model output derived from tainted input**. Workflow steps declare a role: **reader** (no tools, emits typed values), **planner** (typed values in, typed values out), **actor** (typed inputs, raw text only through a gated handle) or **deterministic**. The engine type-checks a workflow at install; letting tainted text reach an actor's prompt is an install error.

**Where roles run.** A reader or planner without tools needs no sandbox: a reader is a schema-constrained model call made and validated in the broker ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)), and the engine receives only the typed values. A planner that needs read access to the code runs in a sandbox with a read-only mount, as an actor variant, and returns typed values through the broker ([ADR-0016](0016-agent-runtime-and-broker-protocols.md)).

**What "by construction" covers.** The type check covers the values the engine passes into a step. It does not see inside a sandbox, where a harness such as Claude Code reads repository files and tool output freely. Any actor that reads forge-authored content therefore runs **tainted**, and for harness actors the bounds are the job manifest's taint policy ([ADR-0004](0004-one-signed-manifest-per-job.md)), the sandbox, and the broker's content gate.

**Plugin-authored text is its own class.** It may enter an actor's prompt, so an installed skill can instruct an actor. It never widens the job: what the actor can do is the intersection of the plugin's granted capabilities and the job's manifest ([ADR-0006](0006-plugins-are-skills-and-wasm-components.md)).

## Consequences

Layer 3 holds by construction for what the engine hands a step. For harness actors it holds by manifest, sandbox and gate, so those three must be tested as the real boundary. Workflow authors must define typed schemas for what a reader or planner extracts. The broker still enforces roles at runtime (a reader's key refuses tools) as a second wall. A poisoned skill can steer an actor within its granted capabilities, so the capability surface and the human consent for grants carry the weight here.
