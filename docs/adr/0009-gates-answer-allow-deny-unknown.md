# ADR-0009: Every gate answers allow, deny or unknown

**Status:** Proposed

## Context

A hook RPC error once skipped the ADMIT router and the needs-human guard (!2313), and fail-closed sites had to be listed by hand.

## Decision

All gates share one interface returning allow, deny or unknown. Unknown always holds the task and opens a decision. Holds are first-class states in the event log.

## Consequences

Fail-closed is enforced by the type, not remembered per site. Outages of optional services hold work instead of letting it through, so health and recovery must be visible to the operator.
