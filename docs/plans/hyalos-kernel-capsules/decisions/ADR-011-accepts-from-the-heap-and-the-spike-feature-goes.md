# ADR-011: `accepts` limits come from the heap, and the spike feature goes

**Status:** accepted (developer, 2026-11-25); amended by ADR-033, the limits come from an outcome search, not a heap peak; amended by ADR-038, ADR-0136 and ADR-0137 are superseded by ADR 0138, not in part

**Proposed by:** developer

## Context

MicroPython's heap is never collected, so memory bounds what inputs may hold (Python ADR-008). The spike's `spike-contract` feature, its profiles and its CI job exist only to test the draft contract.

## Options

1. **`accepts` limits chosen without a measurement.** Rejected: a limit set too high lets a valid-looking input end the run `MemoryError` with no notification.
2. **Keep `spike-contract` as a test rig.** Rejected: it keeps spike code and a CI job beside the real check.
3. **Measure the heap first and fix the limits in a design ADR; delete the spike feature in the last slice.** Chosen.

## Decision

The triage contract's `accepts` limits are measured against MicroPython's heap and fixed in a design ADR, each with its measurement and command. The `spike-contract` feature, `hyalos.spike/1` and `/2` and their CI job are removed in the last slice; the spike's records stay as history and no longer replay in a default build.

## Consequences

The MicroPython build keeps `build.sh`'s current container pin and is rebuilt only if needed. ADR-0136 and ADR-0137 are superseded in part by the pack's repository ADR.
