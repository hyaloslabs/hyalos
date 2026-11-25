# ADR-006: `hyalos-profile` carries the `spike-contract` feature too

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-001, whose Decision names the `spike-contract` feature on `task-kernel` and
`hyalos-audit`, and a matching feature on the kernel image. Spike profiles are defined in
`hyalos-profile`, where `lookup` finds profiles (`hyalos-os/profile/src/lib.rs:97`), and the kernel
image builds from that crate. Without a feature there, a spike profile would compile into every
default build, which ADR-001 forbids. Cold-read round 1 found the gap.

## Options

1. **Define spike profiles ungated in `hyalos-profile` and gate only their lookup through
   `hyalos-audit`.** Rejected: a spike profile would then exist in a default build, and the kernel
   image would see it.
2. **Give `hyalos-profile` its own `spike-contract` feature; `hyalos-audit`, `task-kernel` and the
   kernel image's spike features forward to it.** Chosen.

## Decision

`hyalos-profile` has a `spike-contract` feature, off by default. Every spike profile, and its place
in what `lookup` finds, compiles only with it. `hyalos-audit`'s and `task-kernel`'s features forward
to it, and so does any kernel image feature that builds a spike profile. Everything else in
ADR-001 stands.

## Consequences

- A default build of any hyalOS crate knows no spike profile (S-01.4).
- Slice 1 changes `hyalos-os/profile/Cargo.toml`.
