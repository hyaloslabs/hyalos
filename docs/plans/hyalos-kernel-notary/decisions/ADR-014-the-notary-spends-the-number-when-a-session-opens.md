# ADR-014: The notary spends a run's sequence number when a session opens

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** executor (Slice 5, independent review); accepted by the developer on 2026-11-22

## Context

ADR-006 has the notary number the attestations it signed for a run, so that a second ask at a position gets a number that no longer equals the position. The operator reads the provider's reply before the proof, and the notary spent the number only when it signed. An operator could therefore read an answer, leave before the proof, and ask again. The abandoned session was logged `refused dropped`, spent nothing, and the next session was signed with the number equal to the position. Slice 5's review reproduced this against a test notary. It is the re-ask S-03.5 and ADR-006 exist to stop.

## Options

1. **Report the notary's dropped sessions to the audit.** Rejected: the audit would have to trust a private log it cannot verify, and an auditor has no copy of it.
2. **Keep the number at signing and state the limit.** Rejected: the pack's claim that no answer was re-asked would be false.
3. **Spend the number when the notary accepts the session, before it answers `ok`.** Chosen.

## Decision

The sequence number is how many sessions the notary had already opened for that run, starting at 0. The notary saves it after it accepts the release and the run, and before it answers `ok`, so before the operator can see any reply. If it cannot be saved, the notary answers `sequence` and closes, and ask's word is `provider`.

This amends ADR-006's Decision, S-03.5's "how many attestations it had already signed", the Sequence row and the first message in `design.md` §2, and the session states in §4, where the number is now saved at Connected rather than after Verified. Those files are left as written, and this decision is their reading.

## Consequences

- A session left at any point after it opened uses up its number, so its run can no longer produce a passing record.
- An honest run loses nothing it had. The monitor never retries a call, and any failed call already refuses the run.
- A dropped link, a refused server or a failed provider call now also ends the run's chance of passing. The run must be made again under a new intent.
- A session refused for another release spends nothing, because the notary has not accepted its run.
