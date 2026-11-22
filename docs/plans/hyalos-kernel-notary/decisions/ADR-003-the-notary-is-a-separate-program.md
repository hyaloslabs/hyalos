# ADR-003: The notary is a separate program in the same crate

**Status:** accepted (developer, 2026-11-21); amended by ADR-007, the two programs may also share the trust settings and constants both must agree on

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The spike's one tool is both operator and notary, and it reads the notary key from the operator's home directory. S-01.4 requires that the operator's tool has no way to read a notary key, and that this can be shown from its structure, not only from how it is run. S-02.4 requires that two sides on different protocol releases refuse each other.

## Options

1. **One program with a `serve` command.** Rejected: the operator's program would still contain the code that reads and uses a notary key, so S-01.4 could only be shown by trusting how it is invoked.
2. **A separate crate, or a separate repository, for the notary.** Rejected for now: the two sides must run the same protocol release, and two lockfiles would have to be kept in step by hand. Worth revisiting when the notary ships to customers on its own.
3. **Two programs built from one crate and one lockfile, sharing only what both must agree on.** Chosen.

## Decision

The `attest` crate builds two programs. The operator's program asks, presents and verifies. The notary's program serves sessions and is the only source file that makes, reads or uses a notary key. They share the approved request template, the size limits and the release identifier, and nothing else.

## Consequences

- S-01.4 becomes a structural check: no source file of the operator's program names the notary's key.
- One lockfile pins the protocol for both sides, so a mismatch can only come from running different builds, which the release identifier catches.
- The customer builds from the same source tree as the operator for now. Shipping the notary alone is later work.
