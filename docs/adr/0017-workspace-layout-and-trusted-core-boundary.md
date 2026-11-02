# ADR-0017: Workspace layout and the trusted-core boundary

**Status:** Accepted on November 2, 2026, after the workspace skeleton and its checks were built and run in CI (#109). The layout table and dependency rules are a first cut; a crate that straddles a rule is handled by a new ADR.

## Context

[ADR-0001](0001-rust-for-the-trusted-core.md) says one Cargo workspace produces one binary, and the PRD caps the trusted core at 30,000 lines that one person can review end to end. Nothing yet says which code counts toward that cap, where it lives, or which crates may depend on which. `CODEOWNERS` guessed `/crates/` and `/policy/`. Without a rule the cap cannot be measured, the code-owner paths cannot be exact ([ADR-0014](0014-tracker-holds-status.md)), and the separation in [ADR-0002](0002-one-binary-two-processes-sandboxes.md) (nothing a sandbox can reach holds substrate control or the signing key) is only a convention inside one binary.

## Decision

**Where code lives.** Code that runs inside the engine or broker process is the trusted core and lives under `core/`. Everything else (test kits, the simulator and its fakes, build tooling) lives under `crates/` and is not counted. `core/` holds one directory per crate:

| Crate | Role |
| --- | --- |
| `hyalos-types` | Shared domain types: ids, provenance, verdicts, manifests, event envelopes, channel messages. No I/O. |
| `hyalos-log` | The event log: append, hash chain, signing, verification. |
| `hyalos-policy` | Cedar schema, baseline policies and evaluation ([ADR-0008](0008-cedar-for-policy.md)). |
| `hyalos-substrate` | The substrate interface and its drivers ([ADR-0011](0011-pluggable-substrates.md)). |
| `hyalos-engine` | The state machine: projections, gates, job lifecycle. Reaches the world only through traits it is given. |
| `hyalos-broker` | Manifest enforcement, model protocols, forge and egress proxy, parsing of untrusted input. |
| `hyalos` | The binary. Wires the above together and holds the command line. |

Later crates follow the rule, not this table: a crate that runs in the engine or broker process goes in `core/`.

**What counts toward 30,000.** Every non-blank, non-comment line of Rust under `core/*/src/`, from the start of a file to its `#[cfg(test)]` module (clippy requires that module to come last), plus every Cedar policy and schema line under `core/`. Tests in `tests/`, benches and examples are not counted. Third-party crates are not counted, so their number and size are watched separately (`cargo deny`).

**Dependency rules, checked in CI.**

- `hyalos-types` depends on no other workspace crate.
- `hyalos-broker` depends on no workspace crate except `hyalos-types`. The broker never links substrate control, the log or the signing code; it verifies a signed manifest and enforces it.
- `hyalos-engine` does not depend on `hyalos-broker`. The two talk over the private channel, in the messages defined in `hyalos-types`.
- Only the `hyalos` binary depends on both the engine and the broker.
- A crate in `core/` never depends on a crate in `crates/`, except as a dev-dependency.

**Determinism and safety by lint.** `hyalos-types`, `hyalos-policy`, `hyalos-log` and `hyalos-engine` forbid direct use of the system clock and of `HashMap` and `HashSet`, which is the mechanism for the rule in [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md); the fake clock and seeded randomness land with that story. Every `core/` crate forbids `unsafe`, and denies `unwrap`, `expect`, `panic` and `todo` outside tests, because a panic in the broker must fail a job, not the process. The toolchain is pinned to an exact stable release in `rust-toolchain.toml`.

**Review paths.** `CODEOWNERS` lists `core/`, the workspace and toolchain files, the lint and dependency policy, the checks themselves (`crates/xtask`) and the CI workflows. `cargo xtask lines` and `cargo xtask layout` run on every pull request, so a change that breaks the budget or a dependency rule fails before a human reads it.

## Consequences

The cap becomes a number CI reports on every change, and the engine and broker separation is enforced by the build rather than by habit. Banning `HashMap`, `HashSet` and the clock in four crates costs some convenience; a use that is safe is allowed with an `#[allow]` and a stated reason, which a reviewer sees. The line counter is a heuristic, simpler than a parser, so it can be off by a few percent; the margin below 30,000 absorbs that. The layout is a first cut: a crate that turns out to straddle a rule is split or moved by a later ADR, and the dependency table in the checker changes with it.
