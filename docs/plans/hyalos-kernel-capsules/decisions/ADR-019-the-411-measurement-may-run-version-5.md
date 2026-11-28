# ADR-019: The #411 measurement may run version 5 before the pin

**Status:** accepted (developer, 2026-11-25); amended by ADR-023, `live --version 5` in a `test-trust` build and the developer as customer; amended by ADR-023 and ADR-025, a pre-pin default build reads version 5 as inconclusive

**Proposed by:** author (slice); accepted by the developer at the slice checkpoint

## Context

This amends ADR-012 and ADR-016. The measurement (S-08.1) needs the triage task, which exists only at version 5, run live against the real provider. ADR-012 lets only tests write version 5 before the pin, and the pin waits for the measurement (ADR-016).

## Options

1. **Measure with `attest ask` alone, outside a full run.** Rejected: it does not measure a run at the largest accepted input, which S-08.1 asks for.
2. **A developer's live version 5 run in a `test-trust` build, against the real notary and provider, with no record committed.** Chosen.

## Decision

Before the pin, the developer may run `live` at version 5 in a `test-trust` build, for the #411 measurement only. The record it writes is never committed and never handed to a customer. Default builds still write only version 4 and refuse to read version 5 until the pin.

## Consequences

Slice 10 states this exception in its Contracts. A `test-trust` build still trusts the real roots, so the measured sessions are real.
