# ADR-008: Correct why `p256` checks the log and link signatures

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

ADR-005 chose `p256` so that "the log, link and notary signatures all go through one P-256 implementation". That is wrong. The notary signs with secp256k1 (`spikes/task-kernel/attest/src/bin/notary.rs:710`), and TLSNotary checks it with `k256` (`crates/attestation/src/signing.rs:260` at the pinned tag). The reason ADR-005 gave for rejecting `ring` therefore does not hold, so the choice is open again.

The verifier's tree holds `k256` for the notary, `ring` for the provider's certificate chain (`crates/core/Cargo.toml:52`), and `p256` because `tlsn-attestation` depends on it. Only the log and link signatures are P-256 checks that this project's code makes itself.

## Options

1. **`p256`.** Chosen. Its arithmetic is Rust, so the checks this project adds run no new C or assembly, and it is already in the tree. Its cost is that no other signature in the verifier's real path relies on it, so this project's use is its main exercise here.
2. **`ring`.** Rejected: it is widely used for exactly this check and already runs in the verifier's certificate path, but it would put the log and link checks on C and assembly as well.

## Decision

The log and link signatures are checked with `p256`, under the same comparison with `openssl` that ADR-005 requires. This replaces ADR-005's reason for the choice and leaves the rest of ADR-005 in force.

## Consequences

- The verifier relies on three signature implementations: `p256` for the log and links, `k256` for the notary, and `ring` for the provider's certificates.
- The review in #412 must cover or exclude all three.
