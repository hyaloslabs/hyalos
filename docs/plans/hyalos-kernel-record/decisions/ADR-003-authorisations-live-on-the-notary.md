# ADR-003: Authorisations live on the notary, with no new customer key

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The customer needs a way to state a budget that the operator cannot change. The customer already runs the notary on their own machine, under a key the operator never holds (ADR-001 of the notary pack), and auditors already pin that key.

## Options

1. **A separate customer signing key that signs authorisation documents.** The operator carries the document to the notary. Rejected: it adds a second trust root for auditors to pin, and protects nothing extra, because whoever controls the notary can already sign anything.
2. **Reuse the notary's signing key to sign authorisation documents.** Rejected: that key signs TLSNotary attestations through a library we did not write, and using it for a second message format invites cross-protocol confusion.
3. **A local entry on the notary, made by a command only someone on that machine can run.** What the notary then signs into each attestation is the only statement of it. Chosen.

## Decision

`notary authorize` writes one private file per authorisation in the notary's directory. The serving notary reads it when a session names it. The identifier is random and is not a secret: only the enrolled operator can reach the notary, and it can spend only what the customer authorised.

## Consequences

- No new key and nothing new for an auditor to pin.
- `notary authorize` must work while the notary is serving, so it does not take the serving lock. It takes its own lock so two commands cannot give one task the same round.
- Anyone with access to the notary's machine can authorise. That is the limit the notary pack already states: the notary's independence is the customer's own.
