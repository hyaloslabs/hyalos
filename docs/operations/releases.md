# Releases

A release is a static `hyalos` binary for x86_64 Linux and the sandbox image it runs jobs in, published from CI ([ADR-0081](../adr/0081-prebuilt-releases.md)).

## Installing one

You need Docker (or Podman) and `gh`. The repository is public, so any account can read it. Take the release for the version you want (here `v0.1.9`), download it, and check its SHA-256. The second line runs each step only if the one before it passed:

```
v=v0.1.9; d=$(mktemp -d)
gh release download "$v" --repo hyaloslabs/hyalos -D "$d" -p hyalos-x86_64-linux -p SHA256SUMS && (cd "$d" && sha256sum -c SHA256SUMS)
```

If the checksum does not match, install nothing: the download is not what was published.

Do not install v0.1.0: it refuses every job (#280).

While the repository is public, check the binary's provenance too. It must have been built by the release workflow from that tag. This needs `gh` 2.68 or later, for `--source-ref`:

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

1. Bump `version` in the workspace's `Cargo.toml` (and `Cargo.lock`, with `cargo update -w`). Record the red-team reach for the new version in the same change: run `HYALOS_REDTEAM_RELEASE=<version> cargo test -p hyalos-redteam --test corpus`, which fails and prints the row this release must have, and append that row to `crates/redteam/reach.json` ([ADR-0110](../adr/0110-the-red-team-reach-is-recorded-per-release-and-may-not-regress.md)). Merge that. A tag whose version has no row, or a row other than what the corpus reaches, is not released.
2. Wait for `ci` to finish on that commit on `main`. Then tag it from your own terminal. Publishing is irreversible, so this is your decision, and nothing tags a release for you ([ADR-0111](../adr/0111-a-tag-requires-its-evidence-and-a-human-pushes-it.md)):

   ```
   .github/scripts/tag-release.sh
   ```

   It tags `main`'s head as `v` and the workspace's version, but only if that commit has its evidence. The evidence is a `ci` run on `main` in which the simulation, the job spec, both test parts (the contract suites, the red-team corpus and the chaos scenarios) and `build, lint, test` all succeeded. Otherwise it refuses and says which job is missing. It creates the tag locally only, and prints the push:

   ```
   git push origin v0.1.6
   ```

The `release` workflow's first job, `release evidence`, checks the same evidence again for the tagged commit, however the tag was made. It refuses a tag without it, and every other job waits for it. If `ci` was still running, re-run the workflow once it has finished. It also refuses a tag that is not `v` and the workspace's version, one that is not on `main`, and one whose image is published already. It builds and pushes the image, pulls it back by digest, and builds the binary to name it. It attests both while the repository is public. Then it runs one complete job with that binary and that image, and only if it succeeds publishes the GitHub release with the binary and `SHA256SUMS`, with the image's digest in the notes. A pull request that changes the release runs one check job, which builds both as a release would, runs the same job, and can write nothing.

**What the job proves** ([ADR-0107](../adr/0107-a-release-runs-a-complete-job-before-it-publishes.md)): the binary about to be published runs the fix-issue workflow end to end, against a scripted model and a stand-in GitHub, with no secret. It shows that:
- a reader reads the issue as tainted and a planner plans from its typed values;
- Codex runs in the image the binary names by digest, pushes a branch and opens a pull request;
- every job succeeds, the task completes, and no token is left live;
- the binary's own `hyalos log verify` passes.

A binary that refuses every job, as v0.1.0 did, fails it, and nothing is published. It does not show that the binary works with GitHub itself, a real model, Claude Code, gVisor or a microVM. A tag that failed part-way is cut again under the next version.

**Who may tag.** The workflow refuses a release without evidence, but it cannot stop someone pushing a tag. To refuse the tag itself, add a tag ruleset on `v*` in the repository's settings that lets only maintainers create tags. That is a setting, not code, so this repository cannot check it.
