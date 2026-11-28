# ADR-034: The notary's concurrency comes from its code, and Slice 11 re-runs the declared-limit run

**Status:** accepted (developer, 2026-11-27)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 5's re-check

## Context

This amends ADR-025 and ADR-027. ADR-025 has the measurement probe run a busy-session check before `live`. A live second session cannot be timed into the first: `attest ask` reads its stdin before it connects, and `sessions.log` is written only when a session closes. ADR-027 has Slice 11, when the ceiling is below 200, re-run "the heap check" at the new largest input; ADR-033 retired the heap check.

## Options

1. **Design a live second-session probe.** Rejected: new work with no reliable timing, for a figure the code already fixes.
2. **Take the concurrency from the notary's code and its existing test.** Chosen.

## Decision

- The notary's concurrency is 1, taken from `Semaphore::new(1)` (`hyalos-os/attest/src/bin/notary.rs:1320`) and the test `t_link_a_second_connection_during_a_session_is_refused_busy` (`hyalos-os/attest/tests/notary.rs:684-701`), which CI runs. ADR 0140 records both with the test's command; both network paths run that same code. The probes do not measure it live, and no busy-session check runs before `live`.
- When the ceiling is below 200, Slice 11 re-runs Slice 8's declared-limit run on both backends at the new largest input (ADR-033), in place of a heap check.

## Consequences

The concurrency figure rests on code CI tests, not a timed probe. The pin's re-check follows ADR-033's measure.
