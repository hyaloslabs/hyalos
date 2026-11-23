# ADR-006: A copy is confirmed two independent ways, from its own release tag

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

A verifier the operator supplies proves nothing, so a customer needs evidence the operator cannot produce. The repository already publishes the `hyalos` binary with `SHA256SUMS` and GitHub build provenance from a tag a human pushed (ADR-0111). That release needs the whole product's evidence and the workspace's version, and the verifier ships on its own schedule.

## Options

1. **A signature from a Hyalos-held key.** Rejected: it adds a long-lived key to guard and distribute, and asks the customer to trust its holder.
2. **GitHub provenance alone.** Rejected: the customer must trust GitHub and this repository's release pipeline.
3. **A reproducible build alone.** Rejected: every customer would need the toolchain and a build before their first check.
4. **Both provenance and a reproducible build.** Chosen. The quick check needs only GitHub's CLI, and the strong check trusts no build service.
5. **Reuse the product's `v*` release.** Rejected: a verifier fix would need a full product release.

## Decision

- A separate workflow publishes `hyalos-verify` for a `verifier-v*` tag on `main`, with a static x86-64 Linux binary, `SHA256SUMS` and build provenance.
- It first requires its commit's `hyalos-os` CI evidence, and follows ADR-0111: no workflow creates a tag, and only a human pushes one.
- The documented rebuild, from the tagged source with the pinned Rust toolchain and a pinned C compiler, must produce the published SHA-256 in any directory. CI builds twice in different directories and compares. The C compiler is part of the pin because `ring` compiles C with `musl-gcc` for a static binary (`.github/workflows/release.yml:36`), so the rebuild runs in a container image named by digest.

## Consequences

- Agents treat `verifier-v*` tags exactly as they treat `v*` tags and never push one.
- `crates/xtask/tests/release_gate.rs` must cover the new workflow.
- The first real release is the operator's step, after this pack's code merges.
- If a reproducible build proves impossible, that is a stop and a new decision, not a quiet fallback to provenance alone.
