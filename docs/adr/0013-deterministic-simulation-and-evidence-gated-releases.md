# ADR-0013: Deterministic simulation testing and evidence-gated releases

**Status:** Accepted on November 1, 2026, after review against ADR-0003, 0004, 0009 and 0012 and the PRD success metrics. The clarifications from that review are folded in below.

## Context

Much of today's smoke-matrix flakiness came from time and ordering, and the matrix was the only real check for several changes.

## Decision

Run the engine as a state machine under a fake clock, fake substrate and seeded randomness, replaying thousands of fault schedules in CI. The jobs it runs also depend on a forge and a model provider, so a simulated run fakes those too. Each boundary (broker enforcement, substrate drivers, plugin host) has a property-based contract suite. A TLA+ spec covers the job state machine. The red-team corpus and chaos scenarios gate every release.

**Determinism is enforced, not hoped for.** Direct use of the system clock, a thread-local random generator, or unordered iteration where order is observable is forbidden in the engine by lint, so CI fails a change that introduces a hidden source of time or randomness.

**The spec's invariants are checked on every simulated run.** The TLA+ spec starts with these, taken from the accepted ADRs, and each is also expressed as a property the simulation checks, so the spec and the code cannot drift apart unnoticed:

- a task with an open hold has no running job ([ADR-0009](0009-gates-answer-allow-deny-unknown.md));
- a merge to a protected branch happens only after an approval bound to the PR's head commit SHA ([ADR-0004](0004-one-signed-manifest-per-job.md), [ADR-0012](0012-humans-approve-only-irreversible-actions.md));
- an expired manifest never authorizes work ([ADR-0004](0004-one-signed-manifest-per-job.md));
- the projections equal a replay of the log ([ADR-0003](0003-event-log-is-the-database.md)).

**The red-team corpus is evaluated offline.** It runs against the gates on the worst-case assumption that the model complies, which is the PRD's metric, so the result is deterministic and needs no live model.

**Evidence is necessary for a release, not sufficient.** Publishing a release is an irreversible action, so a human decides it ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). A release cannot be tagged without green simulation, contract, red-team and chaos results for its commit, but green results do not tag it.

## Consequences

Bugs reproduce from a seed. The engine's code is constrained by the lint, which costs some convenience. Simulation covers the engine's logic against faked boundaries, so it cannot prove the real sandbox and broker boundary; the contract suites and chaos scenarios carry that. Everything here assumes CI runs on every change, which story S8.7 provides.
