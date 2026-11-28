# ADR-006: Framed inputs under `hyalos.abi/2`, and every new-version task carries a contract

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

`read_inputs` splits on newlines, so a script cannot be one input under the real profile (context §2 B). Closed answer types (ADR-004) already need a new interface and record version. The spike's type checking existed only in Monty.

## Options

1. **A separate `script` channel beside the inputs.** Rejected: a second channel to secure, and a second version bump.
2. **Length-framed input records, in the same bump as typed answers.** Chosen.
3. **Contracts optional under the new version.** Rejected: a task without one would run unbounded.
4. **Type checking before signing.** Rejected: only Monty had it, and the monitor's check is the boundary (Python ADR-008).

## Decision

Input records are length-framed under `hyalos.abi/2` and a new record version that names its interface. Every task of the new version carries a contract as input record 0, the digest guest's included. Records of versions 1 to 4 read, audit and replay as before.

## Consequences

The `abi/1` section is never edited; `abi/2` gets its own section and pinned digest. The task identity already frames every input (probe 4), so what a customer authorises is unchanged in form.
