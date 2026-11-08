# ADR-0081: Prebuilt releases: a static binary that names its image by digest

**Status:** Proposed

## Context

A first run builds `hyalos` with `cargo install`, after installing Rust, and builds the sandbox image cold. On bot1 (#253) that was about 2 minutes for Rust and 4 for the builds. They ran alongside the operator's browser work, so they were not the bottleneck there, but the PRD's 10-minute target (#61) needs a path with no toolchain and no build (#268).

A release has to give the operator the same trust as building from the repository:
- the binary they run is the one CI built from a tagged commit;
- the image a job runs in is the one built with it, and cannot be swapped by retagging.

Today a job's default image is `hyalos-sandbox:latest`, a tag anything on the machine can point anywhere.

## Decision

**A release is a tag `vX.Y.Z`** that matches the workspace's version. Pushing it runs `.github/workflows/release.yml`, which refuses a tag that does not match. A pull request that changes the workflow builds everything and publishes nothing.

**The sandbox image** is built from `images/sandbox/Containerfile` (`--target sandbox`), as for a build from source. It is pushed to `ghcr.io/<owner>/hyalos-sandbox:<tag>` and named from then on by its digest, `ghcr.io/<owner>/hyalos-sandbox@sha256:<digest>`. Its build provenance is attested and pushed to the registry beside it.

**The binary** is `hyalos` built for `x86_64-unknown-linux-musl`, statically linked, so it runs on any x86_64 Linux. It is built in the Containerfile's own pinned Rust image, with `musl-tools` for the C parts (ring and SQLite). The image's reference by digest is built into it, through `HYALOS_RELEASE_IMAGE`, as the default image (`doctor_cmd::DEFAULT_IMAGE`). A value that is not `<name>@sha256:<64 hex>` fails the build. The workflow checks that the binary is static, runs, and names its image. The binary's build provenance is attested.

**The GitHub release** holds `hyalos-x86_64-linux` and `SHA256SUMS`, and its notes give the image's digest.

**What a release binary changes:** only its default image. `hyalos setup`'s `substrate` step then says `docker pull <image by digest>` in place of the build, and `hyalos doctor` checks that image. `--image` still names any other. A build from source is unchanged: its default is `hyalos-sandbox:latest`.

**Installing.** The `set-up-hyalos` skill installs the latest release when there is one for the host (x86_64 Linux):
1. `gh release download` fetches the binary and `SHA256SUMS`, and `sha256sum -c` must pass.
2. Where `gh` has `gh attestation` (2.49 and later), the binary's attestation must verify too.
3. It installs the binary to `~/.local/bin`, and pulls the image the binary names.

Anything that fails falls back to building from source. Only aarch64 hosts build from source for now. They need an arm64 runner, which a private repository does not get without paying.

**While the repository is private**, downloading a release needs `gh` logged in with access to it. Pulling the image needs `docker login ghcr.io` with a token that can read packages, unless the package is made public. That is an operator step, since the token is theirs.

## Consequences

A machine with Docker and `gh` gets a working `hyalos` in seconds, and the image in the time a pull takes. No Rust and no build are needed.

The image a release's jobs run in is fixed by digest, so retagging cannot change it. Moving to a newer image means installing a newer release, or passing `--image`.

The checksum proves the download is whole. Authenticity rests on GitHub's TLS and the release belonging to the repository. The attestation adds that the binary was built by this workflow from the tagged commit, where `gh` can check it. On an older `gh`, such as Ubuntu 24.04's 2.45, the skill says it was not checked.

A release is cut by hand: bump the workspace version, merge, tag, push the tag. Nothing publishes on a merge.
