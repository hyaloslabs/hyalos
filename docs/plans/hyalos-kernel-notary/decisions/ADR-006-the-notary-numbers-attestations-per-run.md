# ADR-006: The notary numbers attestations per run, and the audit requires the number to equal the position

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-002. That decision had the notary put one ever-increasing count in every attestation, and had the audit require a record's counts to be consecutive. A cold reader found that this leaves the first question of a run open: the operator can ask it several times, keep the last answer, and the record's own counts are still consecutive. Nothing before the first kept answer is checked. Re-asking is the lie the count was added to stop.

ADR-002 rejected telling the notary which run a session belongs to, because the notary would keep state for every run for ever. The developer chose that cost over the hole.

## Options

1. **Keep the single count and account for every number in the public ledger.** Each result link would record the numbers its run used, and the audit would check the operator's whole chain for a missing number. Rejected: it makes a second ask visible, as a refused run logged before the good one, and not impossible. An honest lost attestation would also have to be written off in public.
2. **Keep the single count and state the limit.** Every audit report would say the first answer of a run may have been asked more than once. Rejected: the pack's claim that no answer was re-asked would be false.
3. **The operator tells the notary the run's intent hash, and the notary numbers its attestations within that run.** Chosen.

## Decision

When it opens a session, the operator gives the notary the run's intent hash. The notary signs into the attestation that hash and a sequence number: how many attestations it had already signed for that run, starting at 0. It remembers every run it has signed for, and saves the new number before the attestation leaves.

The audit requires, for the answer at each position, that the run the notary signed is the record's own intent hash and that the sequence number equals the position. This replaces the consecutive-count check. The other two checks of ADR-002 stand: the binding inside the attested request, and the session time inside the run's window.

## Consequences

- A second ask at any position, the first included, gets a number that no longer equals the position, so the record is refused.
- Telling the notary a different run does not help the operator: the attestation then names that other run and the audit refuses it.
- The notary learns each run's intent hash. That hash is already public in the log.
- The notary keeps one intent hash and one number per run, for ever. When it may forget a run is left to a later decision.
- The notary cannot check that the hash it was told matches the request, which it never sees. The audit does, by comparing it with the binding and the record.
- As under ADR-002, an attestation that is signed and then lost makes its run unable to produce a passing record.
- S-03.5 in the prd is reworded and gains examples S-03.5.e4 and S-03.5.e5.
