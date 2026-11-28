# ADR-022: Choices settled at the slice checkpoint

**Status:** accepted (developer, 2026-11-25); amended by ADR-032, 0138 to 0140 are accepted by the developer in their slices' last commits

**Proposed by:** author (slice); accepted by the developer at the slice checkpoint

## Context

The Planner left open points DL1, DL2, DL4, DL5 and DL7 to DL9, each with a proposal and a rejected option.

## Options

1. **Test switch for version 5 before the pin:** (rejected) a new test-only feature; (chosen) reuse `hyalos-audit/test-trust`, which `task-kernel/test-trust` turns on.
2. **Repository ADRs:** (rejected) one ADR amended as slices land; (rejected) the pack's `decisions/`; (chosen) `docs/adr/`, from 0138, Proposed, one per decision: the abi/2 and record version 5 ADR (Slice 2), the `accepts` ADR (Slice 8), the ceiling ADR (Slice 10).
3. **Version after the pin:** (rejected) a version 4 option on the commands; (chosen) the default commands write version 5 only, and version 4 pin tests name version 4.
4. **Release notes:** (rejected) `RECORD.md` or `docs/operations/`; (chosen) a new `hyalos-os/CHANGES.md`, the release notes ADR-007 names.
5. **`BadContract`'s gate:** (rejected) reorder the slices; (chosen) remove it in Slice 2, so the reason bytes keep design §2.5's order.
6. **Triage-contract S-09 rows:** (rejected) a provisional triage contract in Slice 7; (chosen) the generator side in Slice 7 and the triage side in Slice 8.
7. **Slice count:** (rejected) merging slices; (chosen) 12 slices.

## Decision

Each chosen option above is how the plan is built.

## Consequences

`hyalos-os/CHANGES.md` is created by the slice that versions the templates (Slice 6).
