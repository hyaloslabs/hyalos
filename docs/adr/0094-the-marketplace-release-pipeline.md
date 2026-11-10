# ADR-0094: The marketplace release pipeline

**Status:** Accepted

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) makes the marketplace pipeline's release workflow the one trust anchor for plugin releases: every release builds reproducibly in CI and is signed keylessly, and Hyalos accepts only that workflow's identity. [ADR-0093](0093-verifying-a-plugin-release-s-sigstore-bundle.md) built the check and pinned the identity `PIPELINE`: `.github/workflows/release.yml` at `refs/heads/main` in `hyaloslabs/marketplace`, issued by GitHub Actions. That repository did not exist, so no release verified and nothing installed from an index.

S7.1 (#42) asks that two independent builds produce the same digest. S7.2 (#43) asks that the pipeline sign keylessly against its workflow identity, and that a signature from any other workflow fail verification.

Three constraints shape it:

- `hyaloslabs/hyalos` is private. The marketplace's CI cannot read it without a stored credential, so it cannot build or run `hyalos plugin pack`.
- The operator chose to keep `hyaloslabs/marketplace` private too. Keyless signing still writes the repository's name, the workflow's path and the commit into Sigstore's public log for every release.
- hivrr, the system Hyalos replaces, signed marketplace bundles with an ed25519 key. Most of its pipeline guarded that key: an environment holding it behind a named human's approval, a check that no other job named it, a token to commit signatures back to `main`, and key rotation.

## Decision

**The repository.** `hyaloslabs/marketplace` holds one directory per skill under `skills/`, and `versions.json` maps each skill to its version. Its `main` accepts only pull requests a code owner approved, with CI passing, and never a force push or deletion. With keyless signing, what reaches `main` is what gets signed, so that ruleset is the trust anchor's protection.

**Its own packer, held to ours by one digest.** `tools/pack.py` writes the archive [ADR-0090](0090-a-plugin-index-served-as-release-assets.md) defines, with the directory rules `hyalos_plugin::skill` applies: plain names, no links, the same limits. Both repositories hold the same fixed skill, `hyalos-reference`, and pin its digest. Here that is `core/plugin/tests/reference_skill.rs`, and there it is `tests/test_pack.py`. A change to either writer fails its own repository's tests. It was checked byte for byte against `hyalos plugin pack` when written. A second implementation of a 20-line format costs less than a credential that lets the marketplace's CI read this repository.

**Two builds, then one signature (S7.1).** On every push to `main`, `release.yml` builds every skill on two runner images (Ubuntu 24.04 and 22.04). The build reads only the repository, with no clock, network or environment. A third job compares the two builds file by file and stops the release if any byte differs. Only then is anything signed, and what is signed is the compared archive.

**Keyless signing in one job (S7.2).** The `sign` job alone has `id-token: write`. It runs cosign `sign-blob` (cosign v3.1.4, installed by an action pinned to a commit). Fulcio issues a certificate for that run, and Rekor logs the signature, so the bundle carries its log entry and inclusion proof (S7.3). The job then verifies each bundle with cosign for the pinned subject, issuer, repository and ref, and requires that verification *fail* for another workflow's identity. Hyalos refusing other identities is shown by ADR-0093's vectors. This shows the bundles the pipeline publishes carry the pinned identity.

**How the workflow may run.** `tools/check_workflows.py` runs on every pull request. It fails if:

- `release.yml` gains any trigger but a push to `main`, such as `workflow_call` or a pull request;
- its default permissions are more than `contents: read`, or any job but `sign` can ask for a token;
- any workflow runs on `pull_request_target`;
- any action is not pinned to a commit.

So the identity cannot be borrowed from inside the repository, and ADR-0093's repository pin stops it being borrowed from outside.

**A release.** The `publish` job writes `index.json` (ADR-0090) listing each skill at the version just built, and publishes it with the archives and bundles as the GitHub release `r<run>`, marked latest. An earlier version stays installable from the release that built it. A skill whose bytes changed under a version already released is refused, both on the pull request (base against head) and at publish (against the latest release's index). Otherwise an update check (ADR-0092) would never see the change. Nothing is published when the index is unchanged.

**What is not copied from hivrr.** No signing key, so no secret scoping, scope guard, push token or rotation. No approval environment per release: merging to `main` already needs a code owner, and ADR-0007 wants releases within policy to publish without a person. A check that the live repository settings (the ruleset, Actions permissions) have not been loosened is worth having, since those settings are now the trust anchor's protection. It needs a token that can read them and a schedule, and is left to its own story.

**Installing from a private marketplace.** GitHub serves a private repository's release assets only to a signed-in client, and Hyalos fetches without one. The operator downloads a release (`gh release download --repo hyaloslabs/marketplace --pattern '*' --dir <dir>`) and installs from the directory, which is an offline bundle (ADR-0090) checked the same way. Installing by `https://` URL works once the repository is public; nothing in Hyalos changes then.

**Evidence.** The first release, `r1` (`hello` 0.1.0), is a vector in `core/plugin/tests/sigstore/marketplace-r1/`. Its index, digest and bundle pass the same checks an install makes, under `Trust::pinned()`: the embedded snapshot and `PIPELINE`, nothing from a test. Its certificate names the subject, repository (`.1.12`) and ref (`.1.14`) pinned. Another identity, and other bytes, are refused.

No event, wire message, manifest or state changes in Hyalos.

## Consequences

- An install from the marketplace verifies end to end against the identity and snapshot this build pins, so index installs work again, by the operator or within install policy.
- The marketplace's workflow file, repository and branch are the identity. Renaming or moving any of them makes every release unverifiable until a Hyalos release pins the new identity.
- Two archive writers exist. The shared digest catches a drift in either. A new archive format (an index version 2) changes both, and both reference digests.
- Reproducibility is shown across two runner images of one provider, not two providers or architectures. A build depends only on Python's standard library and the repository, so that is where a difference could enter.
- While the marketplace is private, installs go through a downloaded directory. Every release still puts the repository's name in Sigstore's public log; the operator accepted that.
- Automated evidence on each release (S7.4), the signed trusted-root snapshot (S7.11), revocation (S7.7) and the settings-drift check are not here.
