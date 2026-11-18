# ADR-0114: A draft is trusted only after it replays from the signed log

**Status:** Proposed

## Context

S9.3 (#59), the last story of E9 (#56): a skill the engine drafted becomes trusted only after replay in simulation. Its acceptance: a draft that fails replay or the capability checks stays tainted. [ADR-0113](0113-skills-drafted-from-successful-runs-are-tainted-and-read-only.md) drafts a skill from a successful run and holds it to read, tainted. [ADR-0003](0003-event-log-is-the-database.md) makes the log the mechanism that audit, replay and learnings share, and [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) makes the engine deterministic, so the same events always fold to the same state.

A draft's files sit in the plugin store, beside the log but outside it. Between drafting and use, a bug, another process or a person could change them, or a draft could be recorded for values the log does not hold. A draft that has not been checked against the log is only what the store says.

The simulator in `crates/sim` is not in the binary ([ADR-0017](0017-workspace-layout-and-trusted-core-boundary.md)). What the binary has is the deterministic engine and the signed log, which is what the simulator drives too.

## Decision

**Replay is the engine folding the signed log into a fresh state, and drafting again from that state alone.** `draft::replay(log, key, store, name)` takes these steps:
1. It checks the log against the engine's public key, link by link, and folds it with `read_state` into a new state: from the latest snapshot and the events after it, never from the running engine's memory.
2. In that replay, it requires:
   - the draft recorded under `name`;
   - its task completed;
   - its task's last job ended `succeeded`.
3. It loads the draft's files from the store by the digest the log recorded.
4. It requires `run.json` to hash to the request its task recorded.
5. It writes the draft again with ADR-0113's pure function, from the workflow that request names (the operator's document, or fix-issue) and the typed values in the replay. It must come out byte for byte the same.

**The capability checks are part of it.** The recorded grant and what the files ask for must both be within `Grant::read_only()`, and a draft may name no MCP server.

**A draft that fails stays tainted, with a reason code:**
- `draft.log_unverified`, when the log does not check out or cannot be replayed;
- `draft.unknown`;
- `draft.not_succeeded`;
- `draft.capabilities`;
- `draft.files_changed`;
- `draft.request_mismatch`;
- `draft.replay_mismatch`.

Nothing is written for a failure.

**A draft that passes is recorded as `skill.trusted { name, digest }`** (schema version 24), written by the engine only. The state accepts it once, for a draft recorded under that name and digest, and refuses anything else with `TrustRefused`. Each draft image keeps the event that trusted it (`StateImage` version 17). `hyalos run` replays each draft right after it writes it and prints the outcome. It is not a decision: it records a check, and it grants nothing.

**Trusted changes provenance, not capabilities.** A trusted draft's text is Hyalos's own, from the workflow's words and the log's values, so it counts as `PluginAuthored`, as any plugin's text does. A workflow may name it as a step's `plugin` ([ADR-0102](0102-composing-skills-components-and-deterministic-steps.md)). A tainted draft may not be named by a workflow, because a step reading a plugin's text treats it as `PluginAuthored`; only `--skill` gives a tainted draft to a job. Either way the job is held to the read-only grant the draft was drafted with. Widening it is a human's `hyalos plugin install --grant`. `hyalos plugin list` shows each draft, its task, its grant, and whether it is trusted.

## Consequences

A draft is trusted only after the log, checked against its signatures, says it is what the run produced, so its files and the log cannot drift apart unnoticed. Replay reads the whole live log once per draft, which compaction keeps small. A draft whose task was compacted away still replays, since the snapshot keeps tasks, jobs and their values ([ADR-0069](0069-jobs-the-driving-agent-starts-as-mcp-tasks.md)). This closes E9: Hyalos learns from its log, drafts what worked, and trusts a draft only after replay, without a new way in.
