# ADR-002: The canary is a separate program and the notary reads only its result

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The notary holds no provider credential today, and #409 requires that to stay true. The existing probe is Python, which a notary machine may not have.

## Options

1. **A new Rust binary `canary` in the `attest` crate, writing a state file the notary reads.** Chosen.
2. **A `notary canary` subcommand.** Rejected: the notary's own binary would then contain credential handling.
3. **Have the notary launch the Python probe.** Rejected: it adds a runtime dependency and puts the credential path in the notary's configuration.
4. **Run the check through a notarized session.** Rejected: the session's prover holds the hidden bytes, which is the operator's position this check must avoid.

## Decision

`canary check` and `canary reset` are a separate program. The only thing shared with the notary is the state file in the notary's directory. The Python probe stays as the operator's manual tool.

## Consequences

The forged request exists in two languages, so the Rust one is built from the same `body` template the Python probe is pinned to. A check holds an exclusive lock, so two checks cannot race past a latch.
