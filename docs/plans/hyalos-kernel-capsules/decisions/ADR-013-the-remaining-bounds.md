# ADR-013: The remaining bounds

**Status:** accepted (developer, 2026-11-25); amended by ADR-032, `accepts.records` is at most 4,092

**Proposed by:** author (spec); accepted by the developer at the spec design checkpoint

## Context

ADR-008 fixed the contract and question at 4,096 bytes and left the other bounds to the design.

## Options

1. **Leave bounds open, checked only by memory limits.** Rejected: an unbounded field is one a verifier cannot size.
2. **The design's values.** Chosen.

## Decision

Framed inputs: at most 4,096 records, each at most 16,777,216 bytes (`RECORD_MAX`). Contract: at most 16 schemas, `id` 0 to 255, names `[a-z][a-z0-9_]{0,31}` and distinct; at most 16 distinct choices of `[a-z0-9_-]{1,32}`; integer bounds inside `i32`; `code` 0 to 2; `accepts.records` 0 to 4,096, `record_bytes` and `bytes` 0 to 16,777,216; `returns.bytes` 0 to 4,096; `questions` 0 to the ceiling. Anything outside is `BadContract` or `BadInputs`.

## Consequences

The parser and the generator share one corpus that tests each bound at its edge. A later bound change is a new ADR and, if it changes what a record may hold, a version bump.
