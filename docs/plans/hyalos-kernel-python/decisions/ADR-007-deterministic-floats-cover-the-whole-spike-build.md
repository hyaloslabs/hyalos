# ADR-007: Deterministic floats cover the whole spike build

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 2

## Context

This amends ADR-002, whose Consequences say that enabling `wasmi`'s deterministic floats "is
confined to the builds that run the profile that needs it, so it cannot change profile 1 or 2
fuel". In `wasmi` 2.0.3, deterministic float results are the Cargo feature `deterministic`
(forwarding to `wasmi_core/deterministic`, which canonicalises NaN results in float operations).
A Cargo feature applies to the whole build, and one monitor build runs every profile it knows on the
process backend, so the setting cannot be confined to one profile. Cold-read round 2 found this.

## Options

1. **Build a second, Monty-only monitor binary with the feature.** Rejected: more build plumbing,
   a second binary to test, and it would still share `hyalos-audit` with the first through Cargo's
   feature unification unless the workspace were split.
2. **Turn on `wasmi/deterministic` for the whole build whenever `spike-contract` builds a profile
   that allows floats, and rely on profiles 1 and 2 refusing float modules at load
   (`floats: false`, `hyalos-os/profile/src/lib.rs:42`), proven by the pinned-transcript tests.**
   Chosen.

## Decision

The slice that adds a float-enabled spike profile turns on `wasmi/deterministic` through the
`spike-contract` features of `hyalos-audit` and `task-kernel`, and through the kernel image's
feature for that profile. A default build does not enable it. Profiles 1 and 2 keep their
transcripts and fuel because they refuse every float instruction at load; the pinned-transcript
tests run with the feature on to prove it. Everything else in ADR-002 stands.

## Consequences

- Slice 4's stop condition reads: stop if any profile 1 or 2 transcript or fuel figure changes with
  the feature on.
- A default build, and every release, is unchanged.
- Capsules, if it adopts floats, inherits this finding: the setting is build-wide.
