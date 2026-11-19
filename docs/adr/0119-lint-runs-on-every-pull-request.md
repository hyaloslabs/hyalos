# ADR-0119: Lint runs on every pull request

**Status:** Proposed. Supersedes [ADR-0105](0105-the-required-check-gathers-ci-s-split-jobs.md)'s rule that a documentation-only change skips `lint`.

## Context

[ADR-0105](0105-the-required-check-gathers-ci-s-split-jobs.md) lets a pull request that changes only documentation skip every job that `build, lint, test` gathers. `lint` is one of those jobs. Since #333, `lint` also runs `cargo xtask docs`, which checks that every `path::name` the security pages cite still exists. That check is about documentation. A documentation-only change could add a broken reference, and the next code change would then fail on it, far from its cause (#344).

## Decision

**`lint` runs on every pull request,** documentation-only ones included. That covers formatting, clippy, `layout`, `lines` and `docs`.

**`build, lint, test` needs `lint` to succeed in every case.** The other gathered jobs keep ADR-0105's rule: `test`, `simulation` and `spec` must all succeed, or, when only documentation changed, all be skipped.

## Consequences

- A broken reference in `docs/security` fails the pull request that adds it.
- A documentation-only pull request now waits for `lint`, a few minutes, mostly clippy. It still skips the tests, the simulation, the model check, the audit, the conformance suite and the proofs.
