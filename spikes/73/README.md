# Spike #73: offline Sigstore verification

Throwaway tooling and test vectors behind [`docs/planning/spike-73-findings.md`](../../docs/planning/spike-73-findings.md). It is **not product code**: not part of the Cargo workspace, not trusted core. The vectors are meant to outlive it, as regression inputs for the real verifier (ADR-0007).

## What is here

| Path | Role |
| --- | --- |
| `vectors/` | Two public cosign releases' `cosign_checksums.txt` and Sigstore bundles (v3.0.1, October 2025; v3.1.4, October 2026), and the trusted-root snapshot taken on November 2, 2026. All Apache-2.0 files from [sigstore/cosign](https://github.com/sigstore/cosign/releases) and Sigstore's public TUF repository. |
| `variants.py` | Builds the test inputs in `run/`: trust configs, a tampered artifact, bundles with one piece of evidence removed. With `history`, downloads every old trusted root from Sigstore's TUF repository (needs a network). |
| `run-python.sh` | The matrix through sigstore-python 4.5.1, every verification in a container with `--network none`. |
| `rust-build.sh`, `run-rust.sh` | The same matrix through sigstore-rust's own `verify_bundle` example, built at a pinned tag. |
| `Dockerfile` | The Python verifier image. |

## Run it

Needs Docker and Python 3.

```bash
cd spikes/73
./run-python.sh history     # builds the image, fetches old trusted roots, runs the matrix
./rust-build.sh             # once; needs a network (clones and builds sigstore-rust)
./run-rust.sh               # the same matrix through the Rust verifier
```

Each line prints `accepted` or `refused: <reason>`. The expected results are in the findings doc; the verification lines run with no network, and the first line of `run-python.sh` shows that.

## Cautions

- The vectors are signed by a Google service-account identity (`keyless@projectsigstore.iam.gserviceaccount.com`), not a GitHub workflow identity.
- Do not sign anything with a Sigstore public-good identity from this repository to make new vectors: the repository name, the identity and the time are written to a public transparency log that cannot be edited.
- `run/` holds generated files and the built Rust binary and is git-ignored.
