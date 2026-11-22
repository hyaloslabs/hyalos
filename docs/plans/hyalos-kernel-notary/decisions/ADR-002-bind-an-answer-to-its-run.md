# ADR-002: Bind an answer to its run with the intent hash, the log's time and the notary's count

**Status:** accepted (developer, 2026-11-21); amended by ADR-006, the notary numbers attestations per run and the audit requires the number to equal the position

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

An attestation proves the provider's server answered a question at some time. It does not say which run the answer was for. That leaves three lies open to an operator: reusing an old attestation in a later run that asks the same question, obtaining an answer before declaring the run, and asking the same question several times during a run and keeping the answer they like.

The notary never sees the plaintext, so it cannot read a run's name out of the request. The public log already signs a time for the run's intent link and its result link. TLSNotary lets a notary add its own signed fields to an attestation.

The third lie was found while writing this decision. Carrying the run's name in the request does not stop it, because every retry would carry the same name.

## Options

1. **Time alone.** Require the session time to fall between the log's times for the run's two links. Rejected as the only measure: it stops the first two lies and not the third, and it does not tie an answer to a position.
2. **Reveal the run's name to the notary and have it refuse a second session for the same run and position.** Rejected: the notary would have to keep state for every run for ever, and an auditor other than the customer could not check that it did.
3. **Carry the intent hash and position inside the attested request, require the session inside the run's window, and have the notary number every attestation it signs.** Chosen.

## Decision

Three checks together bind an answer to its run:

- The attested request carries the run's intent hash and the question's position, in a field the provider accepts and ignores. The audit requires both to match the record.
- The session time must fall between the times the public log signed for the run's intent link and result link.
- The notary puts a count in every attestation it signs: one more than the last. The audit requires the counts of a record's answers to be consecutive, in order of position. A discarded answer leaves a gap, and a gap refuses the record.

## Consequences

- A model call that the provider answers and the operator then discards consumes a count, so the run it belonged to can no longer produce a passing record. A run with any failed call is refused, as it already is.
- The notary must keep its count across restarts, and never go backwards.
- With one notary serving one operator, runs cannot overlap, or their counts would interleave. That matches the prd's assumption of one session at a time.
- It rests on two things not yet tried: the provider accepting the extra field, and the library letting the notary add a signed field. The prd lists both as assumptions with what happens if they are false.
- The prd gains criterion S-03.5 for the count.
