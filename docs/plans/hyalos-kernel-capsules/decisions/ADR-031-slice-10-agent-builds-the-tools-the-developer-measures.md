# ADR-031: Slice 10: the agent builds the tools, the developer measures

**Status:** accepted (developer, 2026-11-27); amended by ADR-035, Slices 1, 2 and 8 wait on the developer the same way; amended by ADR-036, KVM steps proven by CI or by the developer, an unusable signing key, the shrunk bytes

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 4

## Context

This amends ADR-010. ADR-010 made the #411 measurement a slice of its own and rejected folding it into a slice an agent executes. Slice 10 also holds the tools the measurement needs (`live --version 5`, `attest check`, `notary public --dir` and the probe scripts), which an agent can build. The fourth cold-read round found that the pack did not say who finishes the slice. The repository's rule (`CLAUDE.md`) sets a feature's ADR to Accepted in its pull request's last commit before it merges.

## Options

1. **Move the tools into earlier slices**, leaving Slice 10 as the measurement alone. Rejected: it changes three slices for no gain in safety.
2. **One pull request: the agent builds and tests the tools, then the developer measures and accepts.** Chosen.

## Decision

Slice 10 is one pull request. The executor builds and tests the tools and scripts, then writes `blocked: measurement` in the progress file and stops. The developer runs the probes with their own credential, fills in ADR 0140, and sets it to Accepted, with its README row, in the pull request's last commit. The pull request then merges. Slices 11 and 12 wait for it.

## Consequences

No agent runs the measurement or holds the credential. The ceiling ADR is accepted before its slice merges, as the repository's rule requires.
