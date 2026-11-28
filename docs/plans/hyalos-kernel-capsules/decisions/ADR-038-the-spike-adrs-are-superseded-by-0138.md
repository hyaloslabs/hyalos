# ADR-038: ADR-0136 and ADR-0137 are superseded by ADR 0138

**Status:** accepted (developer, 2026-11-27)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 12

## Context

This amends ADR-011, which says ADR-0136 and ADR-0137 "are superseded in part by the pack's repository ADR". Slice 12 deletes the spike rig those two ADRs decided: the `spike-contract` and `spike-profile-2` features, `hyalos.spike/1` and `/2`, `spike.rs`, `spike_contract.rs` and the `hyalos-os-spike` CI job. The repository's ADR statuses are `Proposed`, `Accepted`, `Rejected` and `Superseded by ADR-NNNN` (`docs/adr/README.md:9`), and no repository ADR uses a partial form. CLAUDE.md's documentation rules say superseding an ADR changes only the old one's Status line and its README row, in the same commit.

## Options

1. **Superseded by ADR 0138, with the status the README already lists.** Chosen.
2. **Add a partial status, `Superseded in part by ADR-NNNN`, to the README's list.** Rejected: it adds vocabulary for one case, when the rig both ADRs decided is removed whole.
3. **Leave both Accepted, and let ADR 0138's Context say what it replaces.** Rejected: an ADR whose decision no longer holds would still read Accepted.

## Decision

- In Slice 12's last commit, the Status lines of ADR-0136 and ADR-0137 become `**Status:** Superseded by ADR-0138`, linking `0138-hyalos-abi-2-and-record-version-5.md`, and their rows in `docs/adr/README.md` read `Superseded by ADR-0138`. Nothing else in either ADR changes.
- If Slice 2 recorded another number for ADR 0138 as a deviation, that number is used instead.
- The rest of ADR-011 holds.

## Consequences

Both spike ADRs carry a status the repository already defines, and a reader of either is sent to the ADR that replaced it.
