# ADR-035: Probe failures before a ticket, the digest contract at the pin, and slices that wait on the developer

**Status:** accepted (developer, 2026-11-27); amended by ADR-036, KVM steps proven by CI or by the developer, an unusable signing key, the shrunk bytes

**Proposed by:** author (cold-read); accepted by the developer at the third run's panel

## Context

This amends ADR-027, ADR-030 and ADR-031. ADR-030 settles a probe step that fails after a ticket is spent, but not one that fails before: a build failure, or a session refused at open with `ReleaseMismatch`, `CanaryHeld`, busy or unreachable, which spends nothing. ADR-027 shrinks only the triage contract to the ceiling C when C is below 200, while the plan also shrinks the digest contract and re-pins its guest. ADR-031 sets who finishes Slice 10, but Slices 1, 2 and 8 also wait on a developer step (the KVM legs on a host without `/dev/kvm`, and accepting ADR 0138 or 0139).

## Options

1. **Every failure after step 2 is a failed run.** Rejected: it spends an authorisation on runs that spent nothing.
2. **Read the tickets, then decide.** Chosen.
3. **Leave the digest contract at 200.** Rejected: it would declare more questions than the ceiling allows.
4. **Shrink both contracts.** Chosen.
5. **The executor marks a waiting slice done.** Rejected: a slice would be done before its acceptance holds.
6. **The executor marks it blocked; the developer sets done.** Chosen.

## Decision

- From step 3 on, each probe's `EXIT` trap prints `notary tickets --dir <state>`. A failure with no ticket spent is re-run under the same authorisation and is not listed as a failed run. A failure after a ticket is spent is a failed run, as ADR-030 says.
- When C is below 200, Slice 11 sets the triage and the digest contracts' `questions` and `accepts.records` to C, rebuilds `digest-v5.wasm`, re-runs Slice 3's reproducible-build check on it, and re-pins both contracts' records.
- A slice that waits on a developer step has its progress Status set to `blocked` by the executor, naming the step. The developer does the step and sets `done` in the pull request's last commit before it merges.

## Consequences

An authorisation is spent only on a run that spent a ticket. Both committed contracts stay within the ceiling. No slice is done before the developer's step it needs.
