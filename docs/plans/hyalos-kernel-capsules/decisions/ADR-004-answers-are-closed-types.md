# ADR-004: Answers are closed types

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

`infer` returns one of four labels for schema `0` (context §2 D). The pack needs questions beyond one label. Model text is a tainted type with no way out (ADR-0047, ADR-0059).

## Options

1. **Free-text answers.** Rejected: a free answer can carry an instruction into the next step or an effect's body, and would be the way out of the tainted type.
2. **Three closed types: `bool`, an integer with declared bounds inside `i32`, and a choice from a declared list.** Chosen. They match v1's typed reader step (ADR-0061), the first joint use parent ADR-003 names.

## Decision

Each `asks` schema declares one of `bool`, a bounded integer (bounds inside `i32`, lower not above upper) or a non-empty choice list without repeats. The model never returns text to a guest.

## Consequences

The record, the monitor, the attest tool, the verifier's check and the canary each change (context §2 D, five copies). This needs a new interface version and a new record version (ADR-006).
