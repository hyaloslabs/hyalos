# ADR-028: The free-text client, the digest guest's check and the SDK's surface

**Status:** accepted (developer, 2026-11-25); amended by ADR-043, free-text-ask's branch sends a file's bytes in place of `body(…)` and `attest ask` never calls `approved`

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 2

## Context

This amends ADR-023, ADR-025, ADR-026 and ADR-018. The second cold read found that `free-text-notary` cannot reach the notary's signer, which is private to `notary.rs`, without a `cfg` branch in that shared file, which ADR-025 ruled out. It also found that ADR-018's "returns no digest unless record 0 parses as a contract" cannot be met by a `no_std` guest that has no contract parser, and that S-09.2's "and nothing else" leaves out `read_inputs` and `finish`, which every script needs.

## Options

1. **A `cfg` hook in `notary.rs`, as `forge-ticket` has.** Rejected: a bad notary is not the threat here, since the notary signs blind.
2. **A test-only client, `free-text-ask`.** Chosen.
3. **A small parser in the guest.** Rejected: more guest code for a check the host already makes.
4. **An exact-bytes check in the guest.** Chosen.
5. **Drop `read_inputs` and `finish` from the SDK.** Rejected: scripts would need the raw import.
6. **Name both in S-09.2.** Chosen.

## Decision

- `free-text-notary` is replaced by `free-text-ask`, a test-only binary of `attest` behind the `free-text` feature (`required-features`). It includes the attest tool's `main.rs` through `#[path]` and skips `approved()` under one `#[cfg(feature = "free-text")]` branch there, the only such branch. It talks to a real, unmodified notary, which signs blind, so it models the threat: a bad operator tool and an honest notary. The S-06.5.e2 test runs a stand-in notary keyed with `golden/v5/notary.key`.
- The version 5 digest guest returns no digest unless record 0 equals the digest contract's canonical bytes, held as a constant. The host's `BadContract` check stays the boundary.
- S-09.2: the SDK exposes one ask per schema, one call per granted effect, `read_inputs` and `finish`, and nothing else.

## Consequences

The attest tool's `main.rs` gains one test-only `cfg` branch. A different contract under the digest guest gives no digest, whether or not it parses.
