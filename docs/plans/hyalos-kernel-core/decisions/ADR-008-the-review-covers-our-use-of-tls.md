# ADR-008: The review covers our use of TLS

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 4

## Context

This amends ADR-005, whose scope table gives no position for the TLS and certificate crates. `hyalos-os/attest/Cargo.toml:23-24` depends on `rcgen` and on `tokio-rustls` with `ring`. The attestation tool uses them for the link between its programs, including a certificate check written in this repository (`hyalos-os/attest/src/lib.rs:476`).

## Options

1. **Include the internals of rustls and ring.** Rejected: it adds specialist cryptography work and cost, which ADR-005 chose to avoid for TLSNotary.
2. **Exclude TLS entirely.** Rejected: our own certificate check would go unreviewed, although the link between the notary programs depends on it.
3. **Cover our use, and exclude the crates' internals.** Chosen.

## Decision

The independent review covers hyalOS's TLS configuration and its certificate check. The internals of `rustls`, `tokio-rustls`, `ring` and `rcgen` are excluded, and the review scope says so in one `use` row and one `internals` row for each.

## Consequences

- A review cannot be described as covering the internals of these TLS crates.
- The custom certificate check is in the reviewer's scope.
- These crates follow the same lockfile and advisory checks as every other row.
