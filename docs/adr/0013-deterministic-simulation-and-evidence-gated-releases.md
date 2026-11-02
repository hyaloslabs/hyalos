# ADR-0013: Deterministic simulation testing and evidence-gated releases

**Status:** Proposed

## Context

Much of today's smoke-matrix flakiness came from time and ordering, and the matrix was the only real check for several changes.

## Decision

Run the engine as a state machine under a fake clock, fake substrate and seeded randomness, replaying thousands of fault schedules in CI. Each boundary (broker enforcement, substrate drivers, plugin host) has a property-based contract suite. A TLA+ spec covers the job state machine. The red-team corpus and chaos scenarios gate every release.

## Consequences

Bugs reproduce from a seed. The engine must avoid hidden sources of time and randomness.
