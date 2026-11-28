# ADR-005: MicroPython on both backends, #503 first, and no new profile

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

Python ADR-008 adopts MicroPython and leaves capsules the kernel image's dispatch (#503), floats and the memory limit. Until #503 is fixed every Python task is process-only, which gives up the KVM containment the product claims.

## Options

1. **Build the contract for native guests first and add Python later.** Rejected: it designs the script and runtime rules without the one runtime that needs them.
2. **New float or memory profiles.** Rejected: MicroPython needs neither, and every verifier must support a profile for good; only Monty, not adopted, needed them.
3. **Fix #503 first, in a planned slice with committed KVM evidence, and keep `hyalos.profile/2`.** Chosen.

## Decision

The first slice fixes the kernel image's stack overflow, and MicroPython's committed records replay byte-identically on both backends. No profile is added; the heap limit becomes a stated input limit in `accepts` (ADR-011).

## Consequences

The fix changes trusted kernel-image code and its review scope row. If it changes any pinned transcript or fuel, that is a stop: a changed profile needs a new name (parent ADR-002).
