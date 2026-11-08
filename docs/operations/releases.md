# Releases

A release is a static `hyalos` binary for x86_64 Linux and the sandbox image it runs jobs in, published from CI ([ADR-0081](../adr/0081-prebuilt-releases.md)).

## Installing one

You need Docker (or Podman) and `gh`, logged in to an account that can read the repository. From any directory:

```
cd "$(mktemp -d)"
gh release download --repo hyaloslabs/hyalos -p hyalos-x86_64-linux -p SHA256SUMS
sha256sum -c SHA256SUMS
gh attestation verify hyalos-x86_64-linux --repo hyaloslabs/hyalos
install -D -m 755 hyalos-x86_64-linux ~/.local/bin/hyalos
```

`gh attestation` needs `gh` 2.49 or later. It checks that the binary was built by the release workflow from the tagged commit. Skip that line on an older `gh`; the checksum still proves the download is whole.

If `hyalos` is not found afterwards, `~/.local/bin` is not on your `PATH` yet. Open a new terminal, which adds it on most distributions.

Then pull the image the binary names. `hyalos setup` prints the line under `substrate`:

```
docker pull ghcr.io/hyaloslabs/hyalos-sandbox@sha256:...
```

While the repository is private, the image needs a login that can read packages first:

```
gh auth refresh -h github.com -s read:packages
gh auth token | docker login ghcr.io -u <your GitHub user> --password-stdin
```

That token is yours: run these in your own terminal, not through the agent.

A release's jobs run in that image by default, named by its digest, so no tag on the machine can change it. `--image` still names another, such as one you built yourself.

## Cutting one

1. Bump `version` in the workspace's `Cargo.toml` (and `Cargo.lock`, with `cargo update -w`), and merge that.
2. Tag the merge commit and push the tag:

   ```
   git tag v0.1.0 && git push origin v0.1.0
   ```

The `release` workflow refuses a tag that is not `v` and the workspace's version. It builds and pushes the image, builds the binary to name it by digest, and attests both. Then it publishes the GitHub release with the binary and `SHA256SUMS`, with the image's digest in the notes. A pull request that changes the workflow builds both and publishes nothing.
