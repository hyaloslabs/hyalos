# ADR-0009: Every gate answers allow, deny or unknown

**Status:** Accepted on November 1, 2026, after review against ADR-0004 and ADR-0012. The clarifications from that review are folded in below.

## Context

A hook RPC error once skipped the ADMIT router and the needs-human guard (!2313), and fail-closed sites had to be listed by hand.

## Decision

All gates share one interface returning allow, deny or unknown. Unknown always holds the task and opens a decision. Holds are first-class states in the event log. Each answer records the gate, a digest of its inputs and a reason.

**Gates the broker evaluates per request.** Egress, the content gate, the role check and the manifest check answer the same way. Unknown refuses that one request, and the broker reports it to the engine, which holds the task. No request proceeds on unknown.

**Resolving a hold.** Unknown never becomes allow by itself. A hold resolves when the gate is re-evaluated and can answer, or by a decision at the tier of the action ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)): routine by policy, escalated by the verifier agent, irreversible by a human. A verifier agent cannot resolve a hold that touches credentials, policy, publishing or a capability grant.

## Consequences

Fail-closed is enforced by the type, not remembered per site. Outages of optional services hold work instead of letting it through, so health and recovery must be visible to the operator. A hold does not extend a job's deadline ([ADR-0004](0004-one-signed-manifest-per-job.md)): a job whose deadline passes ends, and its task stays held.
