# ADR-001: hyalOS is its own Cargo workspace in this repository

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (parent-level pass); accepted by the developer

## Context

The parent pack left open whether the kernel and monitor are core, a crate, or a separate repository, and assigned the answer to `hyalos-kernel-engine`, the last pack. Every pack before it builds on the answer, and the verifier needs it first.

Measured on `origin/main` at `9ba830f`:

| Fact | Measure |
|---|---|
| Trusted core (`cargo xtask lines`) | 28,594 of 30,000 lines, leaving 1,406 |
| Spike production code in `kernel`, `attest` and `hyalos`, without tests | About 4,134 lines, by a simpler count than `xtask` |
| `unsafe` | The kernel and monitor need it. The workspace sets `unsafe_code = "forbid"` |
| Locked packages | 289 for `attest`, which brings in TLSNotary from a git tag |
| Target | The kernel is `no_std`, `no_main`, and has its own linker script |

ADR-0017 says a crate that runs in the engine or broker process goes in `core/`. The monitor, notary and verifier need not run in either process.

## Options

1. **Move it into `core/`.** Rejected: it does not fit the budget, it needs `unsafe`, and it would put TLSNotary in the trusted binary.
2. **Add it under `crates/`.** Rejected: `core/` may use `crates/` only as dev-dependencies, so the product could not ship it that way. It would also add these dependencies to the main workspace's lockfile and audit.
3. **A separate repository.** Rejected for now: the format, the audit and the plans still change together, and a second repository adds release and review work before anyone needs it.
4. **Its own Cargo workspace in this repository, at `hyalos-os/`.** Chosen.

## Decision

hyalOS moves out of `spikes/` into a separate Cargo workspace at `hyalos-os/`, with its own lockfile, lint settings, dependency policy and line budget. It builds separate programs: the kernel image, the monitor, the notary and the verifier.

The existing engine reaches hyalOS only across a process boundary. No `core/` crate depends on a `hyalos-os/` crate. If that ever has to change, it needs a new ADR that amends ADR-0017.

The verifier links neither the monitor's KVM code, the notary's service code nor any operator signing code.

## Consequences

- ADR-0017 and the 30,000-line budget stay as they are.
- CI needs a job for the new workspace, and its `changes` filter must learn the path.
- `hyalos-kernel-core` must set the new workspace's line budget and its rules for `unsafe`.
- `hyalos-kernel-engine` no longer decides where the code lands. It decides how the engine calls it.
- Extracting to a separate repository stays possible, because nothing in `core/` links this code.
