# ADR-027: The default instruction and the triage limit under a lower ceiling

**Status:** accepted (developer, 2026-11-25); amended by ADR-029, the version 5 `system` field adds a fixed untrusted-data warning; amended by ADR-034, Slice 11 re-runs the declared-limit run, not a heap check; amended by ADR-035, the digest contract shrinks to C too; amended by ADR-036, accepts.bytes and the digest guest's embedded contract shrink with C

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-017 and ADR-016. The cold readers found that ADR-017's fixed default instruction, used when a schema has no `prompt`, is never written out, though it is pinned into the template digest for good. They also found that a measured ceiling below 200 would leave the triage task's largest accepted input (200 issues, one question each) unable to finish.

## Options

1. **Leave the wording to the executor.** Rejected: it becomes public, pinned bytes.
2. **State the wording.** Chosen.
3. **Floor the ceiling at 200.** Rejected: the measurement exists to set the ceiling.
4. **Shrink triage's accepts with the ceiling.** Chosen.

## Decision

- The default instruction is exactly `Answer the question with one value of the given type.` It is used for every schema with no `prompt`, and is part of every template the `RELEASE` digest covers.
- Before the measurement, the triage contract declares `questions` 200 and `accepts.records` 200. If the measured ceiling C is below 200, Slice 11 sets triage's `accepts.records` and `questions` to C, re-pins the triage contract, SDK and records, and re-runs the heap check at the new largest input.

## Consequences

Changing the default instruction later is a new template version. A lower ceiling shrinks the triage task's batch size.
