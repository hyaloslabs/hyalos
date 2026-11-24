# ADR-0131: A verifier release is confirmed two ways, from a tag a human pushed

**Status:** Accepted

## Context

A verifier the operator hands to a customer proves nothing, so a customer needs evidence the operator cannot produce. ADR-0111 holds the product's `v*` releases to a human-pushed tag and their commit's evidence. The verifier ships on its own schedule and needs only the `hyalos-os` evidence, and `v*` also matched `verifier-v*`.

## Decision

`.github/workflows/release-verifier.yml` publishes `hyalos-verify-x86_64-linux`, `SHA256SUMS` and GitHub build provenance for a tag `verifier-vX.Y.Z`.

- ADR-0111's rules extend to `verifier-v*`. Only a human pushes one, no workflow creates one, and an agent never does. The product's release starts only on `v[0-9]*`.
- The tag must equal `verifier-v` plus the version in `hyalos-os/verify/Cargo.toml` and be on `main`. Its commit must have a green `hyalos-os` job in ci's run on `main` (`release-evidence.sh <commit> hyalos-os`).
- Started by hand on `main`, the workflow runs the evidence, build and provenance, and creates no release. Only the operator starts it.
- Provenance is not optional here. If it cannot be created, nothing is published.
- `.github/scripts/build-verifier.sh` builds in the digest-pinned Rust image with `musl-tools` at a pinned version, mounts the clone at one path and remaps source paths. CI builds each `hyalos-os` change twice in different directories and compares, and builds a copy with one changed byte, which must differ.

A customer confirms a copy either way.

The quick check needs only GitHub's CLI, 2.68 or later. For a published release:

```
gh attestation verify hyalos-verify-x86_64-linux --repo hyaloslabs/hyalos --signer-workflow hyaloslabs/hyalos/.github/workflows/release-verifier.yml --source-ref refs/tags/<tag>
```

For the operator's manual run:

```
gh attestation verify hyalos-verify-x86_64-linux --repo hyaloslabs/hyalos --signer-workflow hyaloslabs/hyalos/.github/workflows/release-verifier.yml --source-ref refs/heads/main
```

The strong check trusts no build service. In any directory, with Docker:

```
git clone --branch <tag> https://github.com/hyaloslabs/hyalos
cd hyalos
.github/scripts/build-verifier.sh dist
cat dist/SHA256SUMS
```

The hash must equal the published `SHA256SUMS`.

## Consequences

- Agents treat `verifier-v*` tags exactly as `v*` tags.
- A release needs the repository to be able to create attestations, so a private repository without that feature cannot publish a verifier.
- The rebuild depends on the pinned image and the pinned `musl-tools` staying available. If either is withdrawn, the pin changes in a new release, and older releases can no longer be rebuilt from those pins.
- CI's `hyalos-os` job takes longer by three clean builds, which must stay inside its time budget.
