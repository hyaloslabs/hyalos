# ADR-007: The two programs may share the trust settings and constants both must agree on

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 7

## Context

This amends ADR-003. That decision said the operator's program and the notary's program share the approved request template, the size limits and the release identifier, "and nothing else". The plan's shared library file also holds settings both sides must agree on: the approved server name, the trusted root certificates, the provider's address and the session deadline. A cold reader found the two in conflict.

ADR-003 was written to keep the notary's key out of the operator's program. The list it gave was narrower than that purpose needed.

## Options

1. **Keep ADR-003 to the letter.** Each program would hold its own copy of the server name, the roots and the deadline. Rejected: two copies of a setting that must be equal can drift apart, and a drift here is a refused session or, worse, a notary that approves a different server from the one the audit expects.
2. **Let the shared file also hold the trust settings and constants both programs must agree on.** Chosen.

## Decision

The shared library file may hold, besides what ADR-003 lists, the trust settings and constants that both programs must agree on. It holds nothing that makes, reads or uses a notary key, and neither does any source file of the operator's program. That rule of ADR-003 is unchanged and stays a structural check.

## Consequences

- One place defines the approved server name, the trusted roots, the provider's address and the session deadline.
- The structural check for S-01.4 covers the shared file as well as the operator's program, as the plan's Slice 2 Verify already does.
- A setting added to the shared file later needs no new decision, as long as it is not key material.
