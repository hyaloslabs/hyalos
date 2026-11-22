# ADR-004: A ticket is spent at a run's first session and never refunded

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The notary must decide when an attempt counts. The notary pack's retro gives the rule: when a count is the defence, ask at which moment the attacker has already seen what they want. An operator sees the provider's reply before the attestation is signed, so a ticket spent any later could be kept by leaving the session after reading it (ADR-014 of the notary pack).

## Options

1. **Spend when the attestation is signed.** Rejected: the operator reads the reply, leaves, and has lost nothing.
2. **Spend at the first session and refund on a provider failure.** Rejected: the notary cannot tell a real provider failure from an operator who broke the session on purpose after reading the reply (ADR-013 of the notary pack).
3. **Spend at the run's first session, durably and before answering `ok`, and never refund.** Chosen.

## Decision

The first session of a new run spends the authorisation's next ticket, in the same durable save as the run's sequence number and before the notary answers `ok`. Later sessions of that run spend no ticket. Nothing returns a spent ticket.

## Consequences

- A provider outage can use up a budget. The customer then authorises again with `--again`, and the raised round shows in every later attestation.
- The ticket and the sequence number share one file and one save, so a crash cannot separate them.
- A run that spends a ticket and never logs a record still counts against the budget, which is the intent.
