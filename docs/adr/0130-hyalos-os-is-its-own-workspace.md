# ADR-0130: hyalOS is its own Cargo workspace

**Status:** Proposed

## Context

ADR-0017 owns the code layout: every crate of the main workspace lives in `core/` or `crates/`, and the trusted core has a line budget. hyalOS began as a spike under `spikes/task-kernel`, as four separate Cargo builds whose tests CI never ran. A customer-facing verifier is about to be built from it, so it needs a tested home.

It cannot join the main workspace. Its kernel image needs `panic = "abort"`, its guest builds for WebAssembly, and TLSNotary brings hundreds of packages and git dependencies that the main workspace's dependency policy refuses. Nothing in `core/` may depend on it.

Putting the monitor and `attest` on one lockfile also forces a choice, because Cargo keeps one of each semver-compatible version and the two had locked different versions of several TLS and cryptographic crates.

## Decision

hyalOS lives in `hyalos-os/`, a Cargo workspace separate from the main one.

- The monitor and `attest` are members and share one lockfile. `hyalos-os/kernel-image` and `hyalos-os/guest` are excluded and keep their own manifests and lockfiles.
- Evidence moves to `hyalos-os/evidence/`, byte-identical.
- The merged lockfile keeps the versions `attest` locked, except where this ADR names a change. `rustls`, `rustls-webpki`, `rustls-pki-types`, `webpki-roots` and `zeroize` take the monitor's newer versions, and `zeroize_derive` follows `zeroize`. `anyhow`, `crossbeam-epoch`, `crypto-bigint` 0.7, `ctutils`, `tokio` and `webpki-root-certs` move to versions that fix published advisories. `tlsn` and `wasmi` do not change.
- `hyalos-os/deny.toml` is the workspace's own dependency policy. It allows TLSNotary's four git repositories, states the licence of the TLSNotary crates whose manifests declare none, and records a reason for each advisory it accepts. The main workspace's `deny.toml` does not change.
- CI's `hyalos-os` job builds `attest`, runs every workspace test and runs that policy. The required check `build, lint, test` needs the job, and accepts its skip only when the change was classified as not touching `hyalos-os/`, `rust-toolchain.toml` or `ci.yml`.
- `/hyalos-os/` needs code-owner review.

## Consequences

- hyalOS's tests run in CI, and a failure blocks a merge.
- Its code does not count against the trusted core's line budget, and it is not in the `hyalos` binary.
- The moved monitor and `attest` code is not reformatted or lint-fixed here, so a rename does not hide a change to security code. That cleanup is later work.
- The workspace accepts three advisories with no compatible fix. `bincode` and `rustls-pemfile` are unmaintained. The timing advisory on `crypto-bigint` 0.5.5, held by TLSNotary's `k256` and `p256`, must be assessed against the notary's signing before any verifier release (#451).
