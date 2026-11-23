# ADR-001: The canary uses the customer's own credential

**Status:** accepted

**Proposed by:** developer

## Context

The probe needs a working provider credential. The operator is the party the check guards against, and controls the hidden bytes of any request it sends.

## Options

1. **The customer's own credential, in a customer-run check.** Chosen.
2. **The operator's credential, as a drift check only.** Rejected: the operator could make the forged request fail, so the result is not evidence.
3. **The operator's check now, the customer's later.** Rejected: #409 could not close on it.

## Decision

The customer runs the canary with a credential of their own. The operator takes no part in it.

## Consequences

Each customer needs a provider account with a credential of the pinned 106-byte shape. Where one person is both operator and customer, a real run shows the mechanism, not independence.
