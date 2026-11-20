# ADR-0124: CI caches builds and runs specialised jobs by path

**Status:** Accepted

## Context

The repository is private, so every runner minute is billed. A full `ci` run costs about 65 to 70 billed minutes: thirteen jobs, each compiling from scratch, plus the `sandbox image` workflow. On 2026-11-19, 79 runs cost 2,061 billed minutes, more than the organisation's monthly allowance, and on 2026-11-20 GitHub stopped starting jobs.

Of that day's minutes:
- The two test parts took 680. About half of each part's time is compiling.
- Four specialised jobs took about 630 together, on pull requests that touched none of their code: Sigstore's library conformance, the Sigstore vectors (musl, and aarch64 under QEMU), MCP conformance and the policy proofs.

## Decision

**Builds are cached.** Every job that compiles Rust restores a cache of its dependencies' builds (`Swatinem/rust-cache`, pinned to a commit like every action here). The cache is saved only from `main`. A pull request restores what `main` built and saves nothing, so no pull request can write what another run restores. Each job keeps its own cache, and the two test parts one each, because their profiles and targets differ.

**Specialised jobs run on a pull request only when something they build changed.** The `changes` job says, for each one, whether the pull request touches its dependency closure, or what every build reads (`Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `.cargo/`, `ci.yml`):

| Job | Runs when the pull request changes |
| --- | --- |
| `policy proofs` | `core/policy/`, `core/types/`, `crates/policy-proof/` |
| `sigstore vectors (musl, aarch64)` | `core/plugin/`, `core/types/`, `crates/sigstore-testkit/` |
| `sigstore library conformance` | `Cargo.lock` |
| `mcp conformance` | anything under `core/`, `crates/mcp-conformance/` |

A job runs unless its answer is `false`. So a push to `main`, where `changes` classifies nothing, runs every job, and so does a `changes` job that failed. A skipped job still reports its check, as for a documentation-only change ([ADR-0105](0105-the-required-check-gathers-ci-s-split-jobs.md)), so `policy proofs` stays a required check.

Nothing else changes:
- The jobs `build, lint, test` gathers always run on code changes.
- A release's evidence ([ADR-0111](0111-a-tag-requires-its-evidence-and-a-human-pushes-it.md)) is a run on `main`, which runs everything.

## Consequences

- A typical pull request costs about half what it did. Most of the saving is the compile time the cache spares, and the specialised jobs on pull requests that do not reach them.
- A break that a specialised job would catch, reached by a path outside its closure, shows only on `main` after the merge. The closures are each job's real dependencies, so that needs a dependency the table does not list. Add one when a crate's dependencies change.
- A cache costs storage, at most GitHub's per-repository limit, which evicts the least recently used entries. A cache that is stale or missing only makes a job slower.
- Making the repository public would make standard runners free. That is a launch decision, not this ADR's.
