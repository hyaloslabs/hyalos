# ADR-0093: Verifying a plugin release's Sigstore bundle with one pinned library

**Status:** Accepted

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) says how an install trusts a release: one check, online or offline, on a Sigstore bundle against a pinned identity and a trusted-root snapshot, by one library in the trusted core, sigstore-rust at an exact version. Spike #73 ran that check with the library's example program. It did not build it into Hyalos, try a GitHub Actions identity or a Rekor v2 bundle, or run on more than one platform.

[ADR-0090](0090-a-plugin-index-served-as-release-assets.md) names each release's bundle in the index and leaves verifying it to S7.12 (#106). Until then an index install is trusted as a local one is. [ADR-0091](0091-the-install-policy-a-capability-ceiling-the-operator-signs.md) lets install policy install from a listed index, and says that once bundles are verified, an install by policy needs one too.

Two questions are left open by those ADRs. Whose identity does this build pin, while the marketplace pipeline (S7.2) does not exist yet? And how does a test install a release it signed itself, when the binary trusts only the pipeline under Sigstore's own root?

## Decision

**The library.** `hyalos-plugin` depends on `sigstore-verify` `=0.15.1`, the version the spike tested, with default features off and `aws-lc-rs` on. That leaves out the TUF client and the Rekor, Fulcio and timestamp-authority clients, so the check has no network code. aws-lc-rs is the library's only crypto backend; writing our own would put more code in the trusted core than the library does. The rest of the `sigstore-*` crates are pinned by `Cargo.lock`. `hyalos_plugin::sigstore` wraps it in about 150 lines.

**The check.** `Trust::verify(artifact, bundle)` accepts a bundle only if every step below holds, with no network:

- The bundle is at most 256 KiB, parses, and has at least one transparency-log entry, each with an inclusion proof. This is checked before the library runs, so it does not depend on what the library does when they are missing.
- The library verifies with a policy naming the subject, as a URI or an email address as it is written, and the issuer, exactly. It checks the signature over the artifact, the certificate chain to an authority the snapshot lists, the certificate at every signed time the bundle proves, its SCT, and the inclusion proof and checkpoint against a log the snapshot lists. There must be at least one signed time, from the log's entry timestamp or an RFC 3161 timestamp. The certificate is checked only at those signed times, never at the local clock's. The clock is read only to refuse a signed time from the future and, inside the library, to choose which of the snapshot's log keys are in force.
- The result must report the certificate, the SCT, the log and the identity as checked, with at least one verified signed time. Anything less is a refusal.
- For a CI identity, the certificate's source repository and ref (Fulcio's `1.3.6.1.4.1.57264.1.12` and `.1.14`) must equal the pinned ones. A missing claim is a refusal. The subject alone is not enough: on GitHub it names the workflow file that ran, and a workflow in another repository that calls that file as a reusable workflow gets the same subject, with its own repository and a ref it chooses.

A refusal is a `Refusal`, one of `sigstore.too_large`, `.malformed`, `.snapshot`, `.identity`, `.digest`, `.signature`, `.certificate`, `.log`, `.time` and `.refused`. The library's error text is never shown: it can carry values from the bundle, which is untrusted.

**The pinned trust.** `Trust::pinned()` is what an install uses, and nothing outside the release changes it:

- The identity is `PIPELINE`: subject `https://github.com/hyaloslabs/marketplace/.github/workflows/release.yml@refs/heads/main`, issuer `https://token.actions.githubusercontent.com`, repository `https://github.com/hyaloslabs/marketplace`, ref `refs/heads/main`. The release workflow runs on the protected main branch of a `marketplace` repository, so the subject does not change per release. S7.2 builds that pipeline. If it settles on another repository or workflow, it changes this constant in the same pull request.
- The snapshot is `core/plugin/trust/trusted_root.json`: Sigstore's production trusted root of November 2, 2026, the spike's. S7.11 replaces it with the pipeline's signed snapshot and its import rules. Until then it changes only with a Hyalos release.

**Install verifies before it reads.** `hyalos plugin install --index` and `hyalos plugin update` fetch the archive, check its digest against the index, fetch the bundle the index names (at most 256 KiB) and verify it against the archive, in that order. Only then is the archive read. A release with no bundle, or one that does not verify, is refused and nothing is written. This holds for an install by the operator and an install by policy alike, so a listed index is no longer trusted by its location alone (ADR-0091).

**Trust is a parameter, not a switch.** `plugin_cmd::manage` builds `Trust::pinned()` and calls `manage_with(args, env, trust, passphrase)`. The binary only ever calls `manage`. No flag, environment variable, file or build feature selects another trust, so no build of the binary can trust a key held in this repository. Tests sign their own releases with `hyalos-sigstore-testkit` (`crates/`, not trusted core): a certificate authority, a CT log and a Rekor v1 log whose keys exist only in the test process, producing v0.3 bundles the real verifier accepts. The tests pass its root to `manage_with` in-process, and also show the real binary refusing those releases.

**Evidence in CI.**

- `tests/sigstore.rs` in `hyalos-plugin` runs on every build, offline:
  - spike #73's matrix: the fresh and year-old cosign releases are accepted. A changed artifact, a wrong identity or issuer, a flipped signature, a removed log entry, a removed inclusion proof and a bundle with no signed time are each refused.
  - A GitHub Actions bundle verifies with its workflow, issuer, repository and ref, and is refused with another repository, another ref, and by the pinned trust.
  - A Rekor v2 bundle verifies against a snapshot listing its log and is refused against one without it. It is from Sigstore's staging instance; no production Rekor v2 bundle was available.
  - A snapshot missing the authority a bundle used refuses it.
  - The test kit's bundles verify against its root and are refused by the pinned trust. One signed by the pipeline's workflow file for another repository, or with no repository, is refused.
- A CI job checks out sigstore-rust at the commit the pinned version's tag names. It runs the library's own tests and Sigstore's conformance suite (`sigstore/sigstore-conformance`, signing tests skipped) against it. The job pins the version and commit, and fails if `Cargo.lock` locks another version, so changing the library means changing that job and re-running both.
- The same vectors run for every platform Hyalos supports: x86_64 Linux in the main build, the static musl target, and aarch64 Linux cross-compiled and run under QEMU, since the repository has no arm64 runner.

No event, wire message, manifest or state changes. An install records what it did before.

## Consequences

- An index install, by the operator or by policy, needs a release the pipeline signed. Until the pipeline exists (S7.2), no release verifies, so nothing installs from an index; local installs are unaffected. That is the fail-closed reading of ADR-0007.
- A compromised index location gains nothing: it cannot produce a bundle the pinned identity signed, so ADR-0091's interim risk ends here.
- The trusted core carries a pre-1.0 library and its crypto backend. The version is exact, a change to it is a change to the CI job, and the vectors and conformance suite run against it.
- Snapshot updates, maximum ages and revocation are still S7.11, S7.6 and S7.7. A Rekor v2 bundle from production, when one is published, should join the vectors.
