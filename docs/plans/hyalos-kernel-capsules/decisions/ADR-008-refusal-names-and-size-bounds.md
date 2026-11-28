# ADR-008: Refusal names and size bounds

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

The new checks need named refusals in the closed `Reason` set and `ABI.md` (context §3), and the contract needs size bounds.

## Options

1. **Reuse `BadContract` and `TooLarge` for every new refusal.** Rejected: one name for several causes hides which check refused.
2. **One new name per cause.** Chosen.
3. **`returns` declares a closed type.** Rejected: answers are already closed, so a result carries no free model text; a size limit is enough.

## Decision

New reasons: `BadInputs` (malformed framed inputs), `InputLimit` (data outside `accepts`), `ResultLimit` (a result over `returns`'s size), `QuestionLimit` (a question past the limit). `BadContract` stays, in the default build, for any unreadable or ill-formed contract, bounds outside `i32` included. The contract and each question stay at most 4,096 bytes; the design fixes the other bounds. A late, unreachable or erroring provider keeps the existing live reason, and no passing record is written.

## Consequences

Each name is appended to `REASONS` and named in the `abi/2` section. The design records any further bound in its own ADR.
