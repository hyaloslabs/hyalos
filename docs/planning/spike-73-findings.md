# Spike #73: verifying a Sigstore bundle fully offline

Working material, run November 1 to 2, 2026. It settled the former open question on offline Sigstore verification. What it settled is folded into [ADR-0007](../adr/0007-capability-based-marketplace-trust.md); this file keeps the evidence and the method.

## Question

Does a Sigstore bundle verify with no network against a trusted-root snapshot we ship, with the same evidence an online install checks? What goes wrong when the snapshot is old? What maximum age should the bundle and the snapshot carry?

## Method

- **Real signatures, not ones we made.** Signing keylessly needs an identity, and doing it from our private repository would write its name into the public transparency log. Instead the vectors are two public releases of [cosign](https://github.com/sigstore/cosign/releases), `cosign_checksums.txt` and its `.sigstore.json` bundle: v3.0.1 (published October 8, 2025) and v3.1.4 (October 20, 2026). Cosign is Apache-2.0.
- **Signer identity.** `keyless@projectsigstore.iam.gserviceaccount.com`, issued by `https://accounts.google.com`. This is a service-account identity, not a GitHub workflow. The comparison is the same (certificate subject and issuer), but a GitHub-workflow bundle was not verified in this spike.
- **Trusted-root snapshot.** The `trusted_root.json` that sigstore-python's TUF client had cached after one online run on November 2, 2026. Old snapshots came from Sigstore's TUF repository (`tuf-repo-cdn.sigstore.dev`), which keeps every version.
- **No network.** Every verification ran in a container started with `--network none`. A connection attempt from inside it fails at name resolution.
- **Two verifiers.** sigstore-python 4.5.1 (`sigstore verify identity --offline`), and sigstore-rust's own `verify_bundle` example, built from tag `sigstore-verify-v0.15.1` (commit `fc308a2f`). Hyalos is Rust, so the second is the one that counts; the first is a cross-check.
- **Rig and vectors** are in [`spikes/73/`](../../spikes/73/).

## Results

### The offline path works

| Case | sigstore-python 4.5.1 | sigstore-rust 0.15.1 |
| --- | --- | --- |
| Fresh bundle (October 2026) | Accepted | Accepted |
| Year-old bundle (October 2025), whose 10-minute certificate expired long ago | Accepted | Accepted |
| Artifact changed by one line | Refused: digest mismatch | Refused: digest mismatch |
| Right bundle, wrong expected identity | Refused: identity does not match | Refused: identity mismatch |
| One bit of the signature flipped | Refused | Refused |

The certificate lives 10 minutes. A year-old bundle still verifies because the verifier checks the certificate against the signed time recorded in the bundle (the log's entry time or a timestamp authority's), never against the local clock.

One verification took about 3 ms in sigstore-python, so install-time cost is not a concern.

### What the bundle must carry

A v0.3 bundle from these releases holds the certificate, a log entry with its Merkle inclusion proof and signed checkpoint, a signed entry timestamp from the log, and an RFC 3161 timestamp. Removing evidence from the fresh bundle:

| Removed | Both verifiers |
| --- | --- |
| The log entry | Refused |
| The inclusion proof | Refused |
| The log's signed entry timestamp only | Accepted (the RFC 3161 timestamp still supplies the time) |
| The RFC 3161 timestamp only | Accepted (the log's timestamp still supplies the time) |
| Both timestamps | Refused: no verified time |

So "install refuses a release with no log entry" holds offline, because the proof is inside the bundle. A bundle needs at least one verified signed time.

### Old snapshots

The trusted root changed on July 20, 2025, October 9, 2025 and May 17, 2026 (the repository's `Last-Modified` for each version; versions 6 to 9 share one upload time, so their dates say nothing). The gaps were 81 and 220 days. Each version against both bundles, offline:

| Snapshot (TUF targets version) | First served | sigstore-python | sigstore-rust |
| --- | --- | --- | --- |
| 6 | Aug 30, 2024 (upload time) | Refused: no timestamp authority | Refused: timestamp authority |
| 7 to 10 | Aug 30, 2024 | Accepted | **Refused**: timestamp verification fails |
| 11 | Jul 20, 2025 | Refused: no timestamp authority | Refused |
| 12 | Oct 9, 2025 | Accepted | Accepted |
| 13 and 14 (current) | May 17, 2026 | Accepted | Accepted |

- Staleness **fails closed**: a snapshot that lacks an authority the bundle used refuses the bundle with a clear error. It did not fall back to a weaker check.
- The oldest snapshot that verifies today's bundles under the Rust verifier is 13 months old. Age alone is not what breaks it; a missing authority is. But a new authority arrives every few months, and after that an old snapshot refuses new releases.
- The two verifiers disagree on snapshots 7 to 10. Python accepted a bundle whose timestamp it could not check against that snapshot's authority; Rust refused it. The Rust behavior is the stricter and is the one to rely on.
- The current snapshot lists a second transparency log (`log2025-1.rekor.sigstore.dev`, the new Rekor v2). No bundle in the vectors uses it (both are logged in the original Rekor), and I could not get one that does: signing one ourselves would publish our private repository's name, and the GitHub attestation bundle I tried to inspect was in a format I could not decode. So a **Rekor v2 bundle was not tested**.

### Why not carry Sigstore's TUF metadata

The TUF `timestamp.json` fetched on November 2 expires on November 8, so about a week of life. A conforming TUF client refuses expired metadata, so copied TUF metadata stops working in an air gap within days. A bare `trusted_root.json` has no signed issue time at all. So the snapshot's age needs a signed time that Hyalos adds, which is what ADR-0007 specifies.

## The Rust library

The `sigstore-*` crates are the official [sigstore/sigstore-rust](https://github.com/sigstore/sigstore-rust) project ("Sigstore implemented in Rust for GitHub v0.3 bundles"), Apache-2.0, with a Sigstore maintainer among the crate owners. The first release was November 27, 2025; version 0.15.1 was published October 23, 2026, after 22 releases. It is pre-1.0 and a year old, and it includes a conformance crate. The older `sigstore` crate (sigstore-rs) describes itself as cosign-style sign and verify; it was not tested.

## Not tested

- A Rekor v2 bundle against a snapshot with and without the new log.
- A bundle signed by a GitHub Actions workflow identity (the vectors use a service account).
- The Rust verifier's behavior on macOS or Windows.
- Timestamp-authority and log-key rotation that removes a key from a snapshot (the snapshots only ever added authorities).
- Building and running the verifier inside the Hyalos binary; the spike ran its example program.
