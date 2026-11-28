# ADR-010: The #411 measurement, its owner, and the live deadline

**Status:** accepted (developer, 2026-11-25); amended by ADR-018, the test hooks are kept out of release by the release verifier's trust; amended by ADR-023, the measurement's trust checks and scripts; amended by ADR-031, the agent builds Slice 10's tools and the developer measures and accepts ADR 0140 in its pull request; amended by ADR-036, concurrency is stated from the notary's code, not measured

**Proposed by:** developer

## Context

The question-limit ceiling and the live deadline rest on what an attested call costs at the largest accepted input. One real call has been measured (context §2 E). Measuring needs the developer's provider credential, which never enters the repository. Tests need a way to produce out-of-type replies.

## Options

1. **One network path only.** Rejected: one process (1.9 s) and two machines (7.42 s) differ fourfold.
2. **Fold the measurement into an implementation slice.** Rejected: an operator-only step would block a slice an agent executes.
3. **Keep today's deadline formula alone.** Rejected: a run at its full limit could time out.
4. **The developer runs the measurement in its own slice, on both paths; the deadline is the larger of today's formula and L × measured call time × 1.5; tests use the `test-trust` stand-in provider with a forced reply and a stand-in delay.** Chosen.

## Decision

The developer runs the #411 measurement with their own provider credential, recording call time, bytes per attestation and concurrency on both paths, each with its command. It is a slice of its own; slices that need its figures wait for it, and others use the provisional ceiling of 200. A live run's deadline is the larger of `max(600, inputs × 35 + 60)` seconds and L × the measured call time × 1.5. The `test-trust` stand-in provider gains a forced out-of-type reply and a delay, and the release-switch test proves a default build has neither.

## Consequences

The ceiling ADR that S-08.2 writes names the measured value and the figures. The 1.5 margin is recorded there with the measurement.
