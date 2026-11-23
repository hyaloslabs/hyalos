# ADR-003: One audit library serves the monitor and the verifier

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The audit lives inside the monitor's `main.rs` and `ledger.rs`, beside the code that runs live tasks (`context.md` §2). The verifier must not link that code, and the report's wording is a public contract.

## Options

1. **Copy the audit into the verifier.** Rejected: two copies of a security check drift, and a fix to one would not reach the other.
2. **Package the monitor and call its `audit` command.** Rejected: the customer would run the code that runs tasks, including KVM.
3. **Extract one library, `hyalos-audit`, that both use.** Chosen.

## Decision

`hyalos-audit` holds the record types, the task identities, replay, the chain and log checks, the answer checks and the report. It has no KVM, live-task, notary-service or signing code. The monitor's `audit` command and `hyalos-verify` are both thin callers of it.

Before extracting, the executor captures the audit's report for each committed record as a golden file. Those files, not the refactored code, define "byte-identical".

`hyalos-verify` exits 0 for a completed pass, 1 for a refusal and 2 for inconclusive. The monitor's own exit codes do not change.

## Consequences

- A fix to the audit reaches both programs at once.
- The monitor's host and the audit's replay share one host-call implementation, so the live path must plug into it without entering the library.
- A wording change to the report fails the golden test and must be deliberate.
