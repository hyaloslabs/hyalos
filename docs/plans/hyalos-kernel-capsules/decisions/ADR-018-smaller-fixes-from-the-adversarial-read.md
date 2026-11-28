# ADR-018: Smaller fixes from the adversarial read

**Status:** accepted (developer, 2026-11-25); amended by ADR-020, the version 4 input rule is for the default commands; amended by ADR-025, a pre-pin default build reads version 5 as inconclusive, refused after the pin; amended by ADR-028, the digest guest compares record 0 with the digest contract's bytes

**Proposed by:** author (spec); accepted by the developer after the adversarial read

## Context

The adversarial read on 2026-11-25 found ten non-blocking gaps (AR4 to AR6, AR8 to AR14). Each fix below rejects leaving the gap as it stood.

## Options

1. **Leave each as designed.** Rejected: each is a way past a promise in the prd or a false claim in the design.
2. **Fix each as below.** Chosen.

## Decision

- A version 4 run refuses `BadInputs` on any input record holding a newline, empty, or not UTF-8, the `hyalos.abi/1` form.
- A version 1 to 4 record holding an `abi` field is refused on read.
- The version 5 digest guest returns no digest unless record 0 parses as a contract; `task` prints the guest hash beside the identity.
- Each code record is at most 16,384 bytes (`SOURCE_CAP`), refused `InputLimit`.
- The test hooks are kept out of release by the release verifier's real-only trust (`audit/src/answers.rs:35-38`) and the Mozilla roots in `attest-check`, not by the release-switch test; a test shows a release `hyalos-verify` refuses a record answered through `standin --answer`.
- `generate.py --check <contract> <sdk>` lets a customer confirm an SDK is the generated one.
- `Host` checks `returns.bytes` at the `return` call. An effect performed before a refusal is not recorded, a stated failure mode.
- The SDK skips a data record that is not UTF-8 before asking; `int` answer types are used by the triage task only after the provider probe passes; S-05.2 also runs the largest input with every answer forced to each choice.
- The prd states the answer channel's capacity: up to 32 bits per integer answer, so about 800 bytes over 200 questions.

## Consequences

Each item is a criterion or example in the prd or a contract in the design.
