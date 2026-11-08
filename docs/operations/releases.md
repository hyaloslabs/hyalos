# Releases

A release is a static `hyalos` binary for x86_64 Linux and the sandbox image it runs jobs in, published from CI ([ADR-0081](../adr/0081-prebuilt-releases.md)).

## Installing one

You need Docker (or Podman) and `gh`, logged in to an account that can read the repository. Take the release for the version you want (here `v0.1.0`), download it, and check its SHA-256. Each line runs only if the one before it passed:

```
v=v0.1.0; d=$(mktemp -d)
gh release download "$v" --repo hyaloslabs/hyalos -D "$d" -p hyalos-x86_64-linux -p SHA256SUMS && (cd "$d" && sha256sum -c SHA256SUMS)
```

If the checksum does not match, install nothing: the download is not what was published.

While the repository is public, check the binary's provenance too. It must have been built by the release workflow from that tag. This needs `gh` 2.49 or later:

```
gh attestation verify "$d/hyalos-x86_64-linux" --repo hyaloslabs/hyalos --signer-workflow hyaloslabs/hyalos/.github/workflows/release.yml --source-ref "refs/tags/$v"
```

GitHub offers attestations for private repositories only on Enterprise Cloud, so a release made while the repository is private has none. It has the checksum only.

Then install it:

```
install -D -m 755 "$d/hyalos-x86_64-linux" ~/.local/bin/hyalos
```

If `hyalos` is not found afterwards, `~/.local/bin` is not on your `PATH`. Run `export PATH="$HOME/.local/bin:$PATH"` in that terminal. On most distributions, logging in again adds it for good, since `~/.profile` adds it only if it existed at login.

Then pull the image the binary names. `hyalos setup` prints the line under `substrate`:

```
docker pull ghcr.io/hyaloslabs/hyalos-sandbox@sha256:...
```

While the repository is private, the image needs a login that can read packages first:

```
gh auth refresh -h github.com -s read:packages
gh auth token | docker login ghcr.io -u <your GitHub user> --password-stdin
```

That token is yours: run these in your own terminal, not through the agent. A token with only `read:packages` is narrower than `gh auth token`, and works in its place.

A release's jobs run in that image by default, named by its digest, so no tag on the machine can change it. `--image` still names another, such as one you built yourself.

A release has no microVM init (`hyalos-vm-init`): to run jobs in a microVM, build it from source as [substrates.md](substrates.md) says.

## Cutting one

1. Bump `version` in the workspace's `Cargo.toml` (and `Cargo.lock`, with `cargo update -w`), and merge that.
2. Tag the merge commit and push the tag:

   ```
   git tag v0.1.0 && git push origin v0.1.0
   ```

The `release` workflow refuses a tag that is not `v` and the workspace's version, one that is not on `main`, and one whose image is published already. It builds and pushes the image, pulls it back by digest, and builds the binary to name it. It attests both while the repository is public. Then it publishes the GitHub release with the binary and `SHA256SUMS`, with the image's digest in the notes. A pull request that changes the workflow runs only check jobs, which build both and can write nothing. A tag that failed part-way is cut again under the next version.
