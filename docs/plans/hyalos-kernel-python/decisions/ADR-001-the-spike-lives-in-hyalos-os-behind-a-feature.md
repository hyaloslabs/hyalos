# ADR-001: The spike's monitor changes live in hyalos-os behind a feature

**Status:** accepted; amended by ADR-006, `hyalos-profile` carries the feature too

**Proposed by:** developer

## Context

#438 and the parent brief put the contract check in "the spike's monitor",
`spikes/task-kernel`. That directory no longer exists: `hyalos-kernel-core` moved the kernel and the
monitor into `hyalos-os/` (ADR-0130), where they are code-owned, reviewed, held to a 12,000-line
budget and a list of `unsafe` files (ADR-0133). Core's brief keeps the task contract and any new
guest call out of hyalOS until capsules. The spike still needs a contract check on both backends,
spike profiles and a kernel image built for them.

## Options

1. **Fork the monitor and kernel image into `spikes/438/`.** Nothing in `hyalos-os/` changes.
   Rejected: about 3,000 copied lines outside review, which drift from the real monitor, so the
   spike's evidence would describe a monitor that capsules will not use.
2. **Change `hyalos-os/` behind a `spike-contract` Cargo feature, off by default.** The contract
   check, the spike profiles, `BadContract`, the spike run command and the kernel image's spike
   builds compile only with the feature, as `test-switches` does today. The interpreter ports, the
   SDK generator, the scripts, the fixtures and the evidence live under `spikes/438/`. Chosen.

## Decision

Every change the spike makes to `hyalos-os/` compiles only with the `spike-contract` feature (on
`task-kernel` and `hyalos-audit`, and a matching feature on the kernel image), and a default build
is proven unchanged. Everything else the spike builds lives under `spikes/438/`.

## Consequences

- The spike's code is reviewed, linted and tested in CI's `hyalos-os` job, and counts against
  hyalOS's budget (about 2,950 lines remain).
- A release build, and every record a release writes, is unchanged; a test proves a default build
  knows no spike profile (S-01.4).
- Slice 1 adds a repository ADR (`docs/adr/`) for the feature, the contract check and the spike
  profiles, since ADR-0134 owns profiles.
- Capsules replaces the minimal check with its own enforcement and either removes the feature or
  promotes what it keeps through its own ADR.
