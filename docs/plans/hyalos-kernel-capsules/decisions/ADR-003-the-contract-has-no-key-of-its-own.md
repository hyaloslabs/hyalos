# ADR-003: The contract has no key of its own

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

The pack brief spoke of a signed contract. Every candidate signer conflicts with an accepted decision: an operator-only signature (the parent's Ruled out), v1's per-job manifest key (ADR-0031), or a customer key (record ADR-003, which put authorisation on the notary with no new key).

## Options

1. **A signature on the contract by the operator, the engine or a new customer key.** Rejected: each conflicts with an accepted ADR, as above.
2. **The contract is an input, bound by the customer's authorisation of the task identity that covers it, and admitted by v1's policy.** Chosen.

## Decision

A contract is input record 0 of its task. The task identity covers its bytes, the customer's authorisation binds that identity, and Cedar policy and the operator's approval decide whether it may run (parent ADR-003).

## Consequences

No key is added. A contract cannot be published or reused as a signed object on its own; a hash of its bytes is enough to recognise it.
