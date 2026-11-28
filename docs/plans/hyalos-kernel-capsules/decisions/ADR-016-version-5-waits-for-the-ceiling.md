# ADR-016: Version 5 waits for the ceiling, and its ceiling is fixed for good

**Status:** accepted (developer, 2026-11-25); amended by ADR-019, the #411 measurement may run version 5 before the pin; amended by ADR-023 and ADR-025, a pre-pin default build reads version 5 as inconclusive; amended by ADR-027, triage's accepts shrink with a lower ceiling

**Proposed by:** author (spec); accepted by the developer after the adversarial read

## Context

This amends ADR-012 and ADR-014's choice on a ceiling below 200. A ceiling lowered after version 5 records exist would make later verifiers refuse records customers already hold.

## Options

1. **Re-pin committed records when the ceiling drops.** Rejected: it covers only the records in the repository, not customers'.
2. **`CURRENT_VERSION` becomes 5 only after the ceiling ADR, and that ceiling is fixed for version 5; a lower one needs version 6.** Chosen.

## Decision

The slice that pins `hyalos.abi/2` and moves `CURRENT_VERSION` to 5 depends on the ceiling ADR from the #411 measurement, and comes after it. The ceiling it records is the version 5 ceiling for good. Readers accept version 5 only from that same commit.

## Consequences

The measurement slice is on the critical path to version 5. Before it, version 5 records exist only in tests.
