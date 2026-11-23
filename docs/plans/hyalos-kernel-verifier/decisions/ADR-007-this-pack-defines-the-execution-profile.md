# ADR-007: This pack defines the execution profile

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** developer

## Context

The parent's ADR-002 (`../../hyalos-kernel/decisions/ADR-002-keep-wasmi-and-name-the-execution-profile.md`) says `hyalos-kernel-core` owns the profile and the verifier consumes it. The planned order puts the verifier before the core pack, and a verifier cannot refuse an unknown profile until one is defined. The developer accepted that this pack's first slices move the code and define the profile.

## Options

1. **Wait for `hyalos-kernel-core` to define the profile.** Rejected: it would reverse the accepted order, and the verifier would wait on the monitor's hardening.
2. **Define a temporary profile here and let the core pack replace it.** Rejected: records would name a profile that later changes meaning.
3. **This pack defines the profile, and the core pack consumes it.** Chosen.

## Decision

This pack defines `hyalos.profile/1` and the single definition both backends and the verifier build from. `hyalos-kernel-core` consumes it, and any later change to a profile's contents is a new profile name. This amends only the ownership line of the parent's ADR-002. Everything else in that ADR stands.

## Consequences

- The core pack's brief must treat the profile as an input.
- The core pack still sets the new workspace's line budget and `unsafe` rules (parent ADR-001). Slice 1 moves the code without changing either.
