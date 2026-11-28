# ADR-043: Wording corrections to ADR-028 and ADR-042

**Status:** accepted (developer, 2026-11-27)

**Proposed by:** author (cold-read round 16); accepted by the developer with two corrections

## Context

This amends the wording of ADR-028 and ADR-042; neither decision changes.

ADR-028 describes `free-text-ask`'s one `#[cfg(feature = "free-text")]` branch as skipping `approved()`. `attest ask` never calls `approved` (only `attest verify` and `attest-check` do; Slice 6), so there is nothing for the branch to skip: what it changes is the request it sends.

ADR-042 speaks of the probe naming "each binary it copies to `SSH_HOST`" and of the variable refusal happening before `live` "opens or creates the ledger", without saying who copies the binaries, where the commit comes from, or which test shows the ledger untouched.

## Options

1. **Restate both in the terms the plan implements** (chosen).
2. **Keep the wording** (rejected): ADR-028's describes a call `attest ask` does not make, and ADR-042's leaves the copy step and the test unnamed.

## Decision

- **ADR-028's wording.** `free-text-ask`'s one `#[cfg(feature = "free-text")]` branch sends the bytes of the file `HYALOS_TEST_FREE_TEXT_BODY` names as the request, in place of the `body(…)` request `attest ask` builds. `attest ask` never calls `approved`. Everything else in ADR-028 stands.
- **ADR-042's wording.**
  - Slice 10's setup step 1, not the probe, copies the `notary` and `canary` binaries to `SSH_HOST`, and writes `.hyalos-notary/commit` beside them with the commit they were built from. The probe prints each binary's SHA-256 and that commit, and ADR 0140's Trust line records them.
  - The probe's step 1 refuses any `HYALOS_TEST_*` or `HYALOS_TRUST_*` variable before `live` runs, so no ledger is opened.
  - The 10.3 test asserts that no `ledger` directory and no ledger key file exist after its refused default-build run.
  - Everything else in ADR-042 stands.

## Consequences

The pack, the design and the PRD describe `free-text-ask`'s branch by what it sends, and name the setup step, the commit file and the test behind ADR-042's binaries and ledger clauses. The bodies of ADR-028 and ADR-042 are unchanged; their Status lines point here.
