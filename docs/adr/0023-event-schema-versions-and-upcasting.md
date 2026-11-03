# ADR-0023: Event schema versions and upcasting

**Status:** Proposed

## Context

[ADR-0003](0003-event-log-is-the-database.md) says schemas are versioned and old events are upcast on read. [ADR-0018](0018-event-log-format-and-verification.md) stores a `schema_version` with every event and hashes it with the payload bytes. [ADR-0019](0019-typed-events-and-in-memory-projections.md) makes the event set closed and refuses any version but the one the build knows. So today a build that changes an event cannot read a log written before the change, and an operator would have to throw their history away to upgrade. Nothing says how an event may change, who is allowed to change it, or what an old build does with a log written by a newer one.

This ADR refines one rule of ADR-0019, which refuses every schema version but the current one: older versions are now read, by the mechanism below. The rest of ADR-0019 stands, including refusing a version the build does not know.

The constraint that decides most of this: the hash covers the stored bytes, and the signature covers the hash. A stored event can never be rewritten, not even by the engine, without breaking verification for it and every event after it. So an old event stays as it was written, forever, and the current build has to understand it as it is.

## Decision

**Old events are upcast in memory, on read. The log is never rewritten.** `Event::decode` takes the row's `kind`, `schema_version` and payload. A row at the current version is decoded as it is today. A row at an older version is parsed as JSON, passed through the upcast steps from its version up to the current one, and then decoded as the current event. A log can hold events of several versions, and `hyalos log verify` does not care, since it never decodes a payload.

**New events are written at the current version.** There is one version number for the whole event set, not one per kind. A change to any event bumps it, and a kind that did not change needs no step: it passes through unchanged.

**An upcast step is a pure function** from one payload to the next version's payload for one kind. It takes only that payload: no clock, no state, no other events, no randomness. A new field gets a constant default written into the step. A change that needs anything else, such as a value that has to be looked up from an earlier event, is not an upcast; it is a new event kind that a later transition emits, with its own rules. A step never renames a kind or changes the `kind` tag, because the log's `kind` column is hashed; decoding checks the tag against the column after the steps, as it does now, and refuses a mismatch.

**What a build refuses, and says why:**
- a version newer than the build knows: the log was written by a newer Hyalos, upgrade. The build does not skip the event, guess, or open the log read-only. There is no downgrade path; restoring a backup into an older build fails the same way, loudly, before anything is projected.
- version 0, which was never valid.
- a step that fails on its input, naming the event.
- after the steps, anything the current type does not accept (unknown or missing field, wrong type), as now.

**The table is checked.** The set of steps is a `Schema` value: the current version and the steps. Opening the engine checks it: no step for an unknown kind, none from version 0 or from the current version or later, no two for the same kind and version. A bad table is refused at startup, not found when an old event is read.

**Releasing a version needs evidence.** For every released version and every event kind there is a pinned payload as that version wrote it, kept in the repository and never edited. A test decodes every one of them under the current schema. A test also fails if a kind or a version has no pinned payload, so a change cannot be released without its history being exercised. Whoever bumps the version adds the new version's payloads and the steps, and the tests prove an old log still loads.

**The engine takes the schema as a value** (`Engine::open_with`), defaulting to the build's own, so the simulator and tests can run a future version against a log written by a past one without editing the real event set.

## Consequences

An upgrade needs no manual step: the new build opens the old log, verifies it, and continues it at the new version.

Every released event shape has to be readable forever. The pinned payloads and steps accumulate, which is the cost of an append-only log that is also the database. Compaction ([ADR-0003](0003-event-log-is-the-database.md), S1.7) is the way to retire old versions: a signed snapshot replaces the events it covers, and a snapshot records the schema version it was written at, so the same rules apply to it.

Downgrading is not supported. An operator who needs to go back restores a backup taken before the upgrade, with the loss of everything since.

One version number for all events means a change to one kind adds a (trivial) pass for the others, and every pinned set grows by a version. That is simpler to reason about and to test than a version per kind, and event volume is low.

Steps work on JSON values, so they can only express changes to what the payload says, not changes to what an event means. A change of meaning needs a new kind.
