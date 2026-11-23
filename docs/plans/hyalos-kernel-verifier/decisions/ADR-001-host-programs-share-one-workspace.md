# ADR-001: The host programs share one workspace, and the kernel image and guest build apart

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The parent's ADR-001 moves hyalOS to its own Cargo workspace at `hyalos-os/`. The spike is four separate builds today (`context.md` §1). Not all four can share a workspace: Cargo applies a profile such as `panic = "abort"` to a whole workspace, the kernel image needs it, and the guest builds for WebAssembly.

## Options

1. **All four in one workspace.** Rejected: the image's `panic = "abort"` would apply to the host programs, and their tests need unwinding.
2. **Keep four separate builds.** Rejected: the monitor and the verifier could resolve different versions of `wasmi`, and replay depends on that version.
3. **One workspace for the host programs, with the kernel image and guest as separate builds under `hyalos-os/`.** Chosen.

## Decision

`hyalos-os/` is a Cargo workspace whose members are the monitor, `attest`, the shared libraries and the verifier, with one lockfile. `hyalos-os/kernel-image` and `hyalos-os/guest` are excluded from it and keep their own manifests and lockfiles.

The moved files keep their history through `git mv`. Evidence files stay byte-identical.

## Consequences

- One lockfile holds `wasmi` and TLSNotary for every host program.
- The kernel image still has its own lockfile and builds `wasmi` with different crate features (`spikes/task-kernel/hyalos/Cargo.toml:10`, `spikes/task-kernel/kernel/Cargo.toml:8`). A test must hold its `wasmi` version to the workspace's, and the byte-identical records in S-02.5 are what show the two builds behave alike.
- Unifying the lockfiles can change resolved versions. The move slice compares them and stops on any change to `tlsn`, `wasmi` or a cryptographic crate.
- Paths that assume today's target directories change (`context.md` §2).
