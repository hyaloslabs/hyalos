# ADR-002: A spike profile per runtime, run on both backends

**Status:** accepted; amended by ADR-007, deterministic floats cover the whole spike build

**Proposed by:** developer

## Context

The KVM backend refuses any guest over 1 MiB (`hyalos-os/monitor/src/kvm.rs:33`), gives the kernel
a 16 MiB heap in 32 MiB of RAM (`kvm.rs:29-41`), and runs only a profile with the settings its
kernel image was built with (`hyalos-os/monitor/src/main.rs:165`, ADR-0134). Monty needs floats,
more than 4 MiB of memory and very likely more than 1 MiB of guest bytes. The parent's ADR-002 says
any change to a profile is a new profile, and floats need `wasmi`'s deterministic float results.
#438's first done-when criterion asks for byte-identical replay on both backends.

## Options

1. **Run Monty on the process backend only.** Record that KVM needs a new kernel layout and leave it
   to capsules. Rejected: criterion 1 would be half met for Monty, and the KVM limits are exactly
   what capsules needs measured.
2. **One spike profile per runtime that needs one, each with a matching kernel image build and, if
   needed, a spike-only machine layout, all behind `spike-contract`.** Chosen.

## Decision

`hyalos.spike/1` has profile 2's settings and limits, with the contract enforced. A runtime that
cannot run under it gets the next `hyalos.spike/N`, whose raised limits (floats with deterministic
results, memory, fuel) are written once and never edited. The kernel image is built per spike
profile, with a spike-only machine layout when the guest or memory needs it. If Monty cannot fit or
boot even then, that is recorded as a finding (S-05.1.e2).

## Consequences

- Every spike profile is a new name; profiles 1 and 2 and their pinned transcripts never change.
- The KVM check in `on_backend` compares against the profile the kernel image in use was built
  with, so a spike kernel image runs only its own profile.
- Enabling `wasmi`'s deterministic floats is confined to the builds that run the profile that
  needs it, so it cannot change profile 1 or 2 fuel (a stop condition in that slice).
- Whether to adopt floats or a higher memory limit stays capsules' decision (parent brief); this ADR
  only lets the spike measure them.
