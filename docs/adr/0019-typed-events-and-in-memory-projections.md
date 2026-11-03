# ADR-0019: Typed events and in-memory projections

**Status:** Proposed

## Context

[ADR-0003](0003-event-log-is-the-database.md) makes the event log the database and says tasks, jobs and holds are projections of it. [ADR-0018](0018-event-log-format-and-verification.md) stores each event's payload as opaque bytes and leaves open what those bytes are. Story S1.2 needs both answered: what an event is, how its payload is encoded, and what "projection" means in code. [ADR-0009](0009-gates-answer-allow-deny-unknown.md) adds that a hold is a first-class state in the log, and [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) requires that replaying a log always gives the same state.

## Decision

**Events are a closed set of typed values**, defined in `hyalos-types`. Version 1 has six: `task.created`, `task.completed`, `job.started`, `job.ended`, `hold.opened` and `hold.resolved`. An event holds typed values, SHA-256 digests and references. It never holds credentials, tokens or raw model or forge text; a request is a digest of a document kept elsewhere.

**The payload is JSON with a `kind` tag** that repeats the log's `kind` column. Decoding refuses an unknown field, an unknown kind, a missing field, a wrong type, a `kind` that disagrees with the column, and any schema version other than the one the build knows. A payload the engine does not understand is an error, never a guess. Reading older versions is S1.4. Each event's exact encoding is pinned by a test, because changing it changes what every log stores.

**An id is the sequence number of the event that created the thing.** A task's id is the sequence of its `task.created` event, a job's of its `job.started`, a hold's of its `hold.opened`. Ids need no randomness, cannot collide, and are the same on every replay.

**Projections are in memory.** `State` is a pure fold over events, kept in ordered maps, with no tables of its own. The engine rebuilds it by replaying the log when it opens, so there is no projection to get out of step with the log, and "delete the projection and replay" is restarting the engine. Replay cost grows with the log until snapshots exist (S1.7), and the pass already made to verify the log at open is the same size.

**Every event is checked against the state before it is written.** `Engine::submit` asks `State::check` first and appends only if the event is legal, so the log never holds an event that replay would refuse. Replay applies the same checks and stops at the first event that breaks them, naming it, which is how a buggy engine or a validly signed but nonsensical event is found instead of becoming wrong state. The rules:

- A job starts only on an open task (not held, not completed) that has no running job. A task has at most one running job.
- A task completes only when it is open and has no running job.
- A hold opens only on a task that is not completed and has no running job. A job whose gate answers unknown therefore ends first (`job.ended` with outcome `held`), and the hold opens after it.
- A task with any open hold is held, and returns to open when its last hold resolves.
- A job ends once, and a hold resolves once. Anything that names a task, job or hold that does not exist is refused.

**Only one job runs per task for now.** Concurrent jobs would need decisions this ADR does not make, such as what a hold means for the sibling jobs. The rule can be relaxed by a later ADR. Tightening it later would invalidate existing logs, which is why it starts strict.

**Dependencies.** `hyalos-types` takes `serde` and `serde_json`, by reviewed entries in `cargo xtask` ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)). Encoding is a pure function of the value. `hyalos-types` now also owns the hex helpers that `hyalos-log` used to carry.

## Consequences

The log can be replayed to the same state on any machine, and a test builds random event sequences from seeded streams, checks the invariants after every step, and compares the live state with a replay.

Adding an event kind, changing a field, or changing the version is a deliberate act: the pinned encodings fail first. Event kinds that later stories need (policy decisions, manifests, gate answers beyond the hold they cause) are added by those stories, each with its transition rules.

The engine holds all tasks, jobs and holds in memory. That is fine for the sizes v1 targets and is bounded by snapshots and compaction (S1.7), but it is a real limit and not a design for millions of tasks. A projection that is queried rather than held would be a new decision.

The rule that a held job must end before its hold opens makes the order of events in the log tell the story: the job's end, then the hold, then the resolution. It also means the engine must end the job before it can record why work stopped, which the substrate and gate stories have to respect.
