# ADR-011: A record cannot change version, and a legacy pass reads differently

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec), from the pack's adversarial read (F7, F8, F12); accepted by the developer at the spec ADR checkpoint

## Context

This amends ADR-005. An operator can wrap attestations that carry a ticket in version 1 links and a version 1 record, so `--legacy-v1` ignores the ticket while the report still opens with `audit passed`. In the other direction, nothing in a transcript depends on its version, so a version 1 record relabelled as 2 replays as identical. Links outside the audited run are not version-checked at all.

## Options

1. **Rely on the report's one legacy line.** Rejected: a script reads the first line, and S-01.3.e2 makes it safe to always pass the flag.
2. **Remove `--legacy-v1`.** Rejected for the reason in ADR-005: it orphans real evidence.
3. **Tie each version to content that only it can carry.** Chosen.

## Decision

- A version 1 audit refuses any attestation carrying `hyalos.ticket/1`.
- A legacy pass opens with `legacy audit passed: no attempt budget`, never `audit passed`.
- A version 2 transcript has a required `task` field, the version 2 identity, which replay recomputes. A relabelled version 1 record therefore fails.
- Every link in a chain must have `v` 1 or 2, and each result's `v` must equal its intent's.

## Consequences

- A script can tell a legacy pass from a budgeted one by its first line.
- The committed chain, all at `v` 1, stays valid.
