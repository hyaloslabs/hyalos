# ADR-001: The notary signs the ceiling and the attempt into each attestation

**Status:** accepted (developer, 2026-11-22); amended by ADR-008, an earlier attestation can understate once a task is authorised again

**Proposed by:** author (spec); accepted by the developer at the spec acceptance checkpoint

## Context

#408 proposed that the notary sign how many tickets it issued for a task, so an auditor could see the number of attempts. A signed count is true only when it was signed. An operator who tried three times can still hold the statement signed after the first try and show that one, so the auditor would also need to know that the count is the latest.

## Options

1. **A signed running count, published to the public log.** The auditor takes the latest entry under the notary's key. Rejected: it adds a second log identity, a write to the public log per attempt, and a freshness rule, all to report a number the ceiling already bounds.
2. **A signed running count, fetched from the notary at audit time.** Rejected: an audit would need the customer's notary to be online and reachable, and the audit is meant to work for a stranger, offline apart from the public log.
3. **The ceiling and the attempt number, inside each attestation.** The notary adds one extension holding the authorisation, the task, the budget, the round and the ticket, under the signature it already makes. Chosen.

## Decision

Every attestation carries a `hyalos.ticket/1` extension with the authorisation, task, budget, round and ticket. The audit reports "attempt k of at most N" from it.

## Consequences

- No new key, no new log write and no freshness rule. An old attestation cannot understate anything, because the ceiling does not change.
- The report states a ceiling, not how many attempts were really used. An operator with a budget of 5 may have used 1 or 5. A customer who wants a single try sets a budget of 1.
- The customer can still read the real number of spent tickets on their own notary.
