# Sigstore test vectors (S7.12)

Real public Sigstore bundles for `tests/sigstore.rs`, which checks the install's verifier against them offline (ADR-0007, ADR-0093). The third-party ones are Apache-2.0; `marketplace-r1/` is this project's own. Do not regenerate or re-sign them: they are evidence, and signing new ones with a public-good identity writes the repository's name into a public log.

| Path | What it is | From |
| --- | --- | --- |
| `cosign-v3.1.4/` | `cosign_checksums.txt` and its bundle, October 20, 2026: the fresh release | [sigstore/cosign](https://github.com/sigstore/cosign/releases), via spike #73 (`spikes/73/vectors/`) |
| `cosign-v3.0.1/` | The same for October 8, 2025: the year-old release whose certificate expired long ago | The same |
| `github/signed-package-2.1.0-hb0f4dca_0.conda` | A conda package | sigstore-rust `sigstore-verify-v0.15.1`, `crates/sigstore-verify/test_data/bundles/` |
| `github/conda-attestation.sigstore.json` | Its attestation, signed by a GitHub Actions workflow (`prefix-dev/sigstore-example`, `refs/heads/main`) and logged in Rekor | The same |
| `github/conda-attestation-rekor2.sigstore.json` | An attestation for the same package logged in Rekor v2, on Sigstore's staging instance (`log2025-alpha3.rekor.sigstage.dev`) | The same |
| `marketplace-r1/` | `index.json`, `hello-0.1.0.skill` and its bundle: the Hyalos marketplace's first release, signed by its pipeline (`release.yml` at `main` in `hyaloslabs/marketplace`) on November 9, 2026 | [hyaloslabs/marketplace](https://github.com/hyaloslabs/marketplace) release `r1` (ADR-0094) |

The cosign releases are signed by a Google service account (`keyless@projectsigstore.iam.gserviceaccount.com`). The production trusted root they verify against is the snapshot the plugin crate embeds (`core/plugin/trust/trusted_root.json`, taken November 2, 2026). The Rekor v2 bundle verifies against the staging trusted root sigstore-rust embeds (`SIGSTORE_STAGING_TRUSTED_ROOT`). No public-good bundle logged in production Rekor v2 was available when this was written.
