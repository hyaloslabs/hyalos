# ADR-005: Signatures are checked with the pure-Rust `p256` crate

**Status:** accepted (developer, 2026-11-23); amended by ADR-008, which corrects the reason for choosing `p256`

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The audit runs an external `openssl` for every link and log signature, through files in a temporary directory (`context.md` §2). A decoy `openssl` on `PATH` could approve anything, and the verifier must write no files. Both candidates are already in the verifier's tree, so neither adds a package. `tlsn-attestation` uses `p256` to check the notary's signature (`crates/attestation/src/signing.rs` at the pinned tag). `tlsn-core` requires `rustls-webpki` with its `ring` feature for the provider's certificate (`crates/core/Cargo.toml:52`), so `ring`'s C and assembly stay in the verifier whichever is chosen here.

## Options

1. **Keep calling `openssl`.** Rejected: the answer would depend on a program outside the verifier.
2. **Link OpenSSL statically.** Rejected: it adds a large C library to a static, reproducible build.
3. **Use `ring`.** Rejected: it would work, but the notary's P-256 signature would still go through `p256`, leaving two P-256 implementations deciding the verifier's signature checks.
4. **Use `p256`.** Chosen: the log, link and notary signatures all go through one P-256 implementation.

## Decision

The verifier checks ECDSA P-256 signatures over SHA-256, in DER form, with `p256` at the version the lockfile already holds. Before `openssl` is removed from the audit path, a test compares both on every stored signature and on altered copies. Any disagreement is a stop.

## Consequences

- The verifier needs no external program and no temporary files.
- The verifier is not free of C. `ring` still checks the provider's certificate chain, and removing it would mean changing TLSNotary.
- `p256` has not been independently audited by this project. It joins the components the review in #412 must cover or exclude.
- The operator's signing path in the monitor still uses `openssl`, because only the audit path changes here.
