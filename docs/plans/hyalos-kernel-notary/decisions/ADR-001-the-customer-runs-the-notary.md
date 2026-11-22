# ADR-001: The customer runs the notary

**Status:** accepted

**Proposed by:** developer

## Context

A notary's attestation is worth exactly as much as the notary's independence from the operator. In the spike the operator holds the notary key, so the attestation convinces nobody else. Someone other than the operator must run it, and the choice decides what "independent" means for the first version.

## Options

1. **An auditor runs it.** A third firm witnesses every call. Rejected for the first version: it needs a third party before there is a first customer.
2. **A public notary service that we run, for any caller.** Rejected for the first version: we cannot be the independent notary for work we run ourselves, and nobody has a reason yet to trust our key. Recorded in the parent pack as a separate idea, priced per attestation.
3. **Two or more notaries, every one required.** Rejected for the first version: it defends against a notary that colludes with the operator, which is not the threat when the customer is the notary.
4. **The customer runs it, on their own machine, under their own key.** Chosen.

## Decision

The first version's notary is a service the customer runs themselves. The operator never holds its key. The developer stated this on 2026-11-21.

## Consequences

- The proof convinces the customer, who is also the party it is for. It convinces an outsider only as far as they trust the customer's notary.
- The customer has something to install and keep running. If it is down, the operator's tasks do not run.
- A notary that colludes with the operator is out of scope: the customer would be deceiving themselves.
- Measuring the notary's cost in this pack gives the public-service idea its first number.
