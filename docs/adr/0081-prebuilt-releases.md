# ADR-0081: Prebuilt releases: a static binary that names its image by digest

**Status:** Accepted; its rule that the workflow checks only that the binary is static, runs and names its image, and its pull-request check jobs, superseded by [ADR-0107](0107-a-release-runs-a-complete-job-before-it-publishes.md)

## Context

A first run builds `hyalos` with `cargo install`, after installing Rust, and builds the sandbox image cold. On bot1 (#253) that was about 2 minutes for Rust and 4 for the builds. They ran alongside the operator's browser work, so they were not the bottleneck there, but the PRD's 10-minute target (#61) needs a path with no toolchain and no build (#268).

A release has to give the operator the same trust as building from the repository:
- the binary they run is the one CI built from a tagged commit;
- the image a job runs in is the one built with it, and cannot be swapped by retagging.

Today a job's default image is `hyalos-sandbox:latest`, a tag anything on the machine can point anywhere.

## Decision

**A release is a tag `vX.Y.Z`** that matches the workspace's version, on a commit on `main`. Pushing it runs `.github/workflows/release.yml`, which refuses any other tag, and a tag whose image is published already: each tag is published once. A pull request that changes the release runs only check jobs, which build the image and the binary with read-only permissions and publish nothing. The publish jobs run only on a tag.

**The sandbox image** is built from `images/sandbox/Containerfile` (`--target sandbox`), as for a build from source. It is pushed to `ghcr.io/<owner>/hyalos-sandbox:<tag>`. From then on it is named by the digest the registry serves for it (`docker buildx imagetools inspect`), `ghcr.io/<owner>/hyalos-sandbox@sha256:<digest>`. The binary's job pulls it back by that digest before building.

**The binary** is `hyalos` built for `x86_64-unknown-linux-musl`, statically linked, so it runs on any x86_64 Linux. It is built in the Containerfile's own pinned Rust image, with `musl-tools` for the C parts (ring and SQLite). The image's reference by digest is built into it, through `HYALOS_RELEASE_IMAGE`, as the default image (`doctor_cmd::DEFAULT_IMAGE`). A value that is not `<name>@sha256:<64 hex>` fails the build. The workflow checks that the binary is static, runs, and names its image (`.github/scripts/build-release-binary.sh`).

**Provenance.** While the repository is public, the build provenance of the image and of the binary is attested, with `actions/attest-build-provenance`. The image's attestation is pushed to the registry. GitHub offers attestations for private repositories only on Enterprise Cloud, and `hyaloslabs` is on Team. So while the repository is private, a release has its checksums and no attestation, and the workflow skips the attest steps.

**The GitHub release** holds `hyalos-x86_64-linux` and `SHA256SUMS`, and its notes give the image's digest.

**What a release binary changes:** only its default image. `hyalos setup`'s `substrate` step then says `docker pull <image by digest>` in place of the build, and `hyalos doctor` checks that image. `--image` still names any other. A build from source is unchanged: its default is `hyalos-sandbox:latest`.

**Installing.** The `set-up-hyalos` skill installs a release when there is one for the host (x86_64 Linux). It takes the release for the checkout's version, so the skill and the binary match:
1. `gh release download` fetches the binary and `SHA256SUMS`, and `sha256sum -c` must pass.
2. Where provenance can be checked (a public repository, and `gh` 2.68 or later, which has `--source-ref`), `gh attestation verify` must pass. It is pinned to the release workflow and the tag: `--signer-workflow …/release.yml --source-ref refs/tags/<tag>`. Where it cannot be checked, the skill tells the operator so, and why.
3. It installs the binary to `~/.local/bin`, and pulls the image the binary names.

A failed checksum or attestation installs nothing and is reported to the operator as a possible tampering sign. No release for the version means building from source. Only aarch64 hosts build from source for now. They need an arm64 runner, which a private repository does not get without paying.

**While the repository is private**, downloading a release needs `gh` logged in with access to it. Pulling the image needs `docker login ghcr.io` with a token that can read packages, unless the package is made public. That is an operator step, since the token is theirs.

## Consequences

A machine with Docker and `gh` gets a working `hyalos` in seconds, and the image in the time a pull takes. No Rust and no build are needed.

The image a release's jobs run in is fixed by digest, so retagging cannot change it. Moving to a newer image means installing a newer release, or passing `--image`.

The checksum proves the download is whole. Authenticity rests on GitHub's TLS and the release belonging to the repository. Once the repository is public, the attestation adds that the binary was built by the release workflow from the tag. Until then, and on a `gh` older than 2.68, such as Ubuntu 24.04's 2.45, the skill says it was not checked.

A release carries no microVM init (`hyalos-vm-init`), so a microVM still needs a build from source.

`~/.local/bin` is on `PATH` on most distributions only after the next login, if it did not exist before. Until then the skill runs `hyalos` by its full path and tells the operator to `export PATH`.

A release is cut by hand: bump the workspace version, merge, tag, push the tag. Nothing publishes on a merge. A tag that failed part-way is not run again; it is cut again under the next version.
