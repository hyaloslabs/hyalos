# ADR-042: The measurement's clean-up, its Trust line and its re-runs

**Status:** accepted (developer, 2026-11-27); amended by ADR-043, setup step 1 copies the binaries and writes their commit, the probe's step 1 refuses before `live` runs, and the 10.3 test shows no ledger

**Proposed by:** author (cold-read round 16 batch); accepted by the developer before round 16's fixes

## Context

This amends ADR-039, ADR-040 and ADR-041. The round 16 cold read found seven places where Slice 10's #411 measurement left a step open: what setup step D removes besides the two credential copies, and whether it runs if the measurement is abandoned; which clocks the Trust line relies on; who authorises the identity the probe prints; where `OPERATOR_TRANSPORT_PUB` comes from; what a re-run after a failed run starts from; when `live --version 5`'s variable refusal happens relative to the ledger; and how the binaries copied to the notary host are named.

## Options

The developer answered each question directly; the alternatives were leaving each step to the developer at the moment it arises (rejected: it is the gap the panel found).

## Decision

- **Step D's scope.** Step D also deletes each path's notary state directory (`SSH_HOST:.hyalos-notary/state` and `NOTARY_STATE`) and the `notary` and `canary` binaries copied to `SSH_HOST`, and every kept failed-run `MEASURE_HOME`. It also runs if the measurement is abandoned before ADR 0140 is Accepted. It stays one progress-file line per measurement path (two lines). Each line names all of that path's removed paths: the one-process line names its `MEASURE_HOME`'s two copies and `NOTARY_STATE`; the two-machine line names its `MEASURE_HOME`'s two copies, the `SSH_HOST` credential copy, `SSH_HOST`'s notary state and the binaries; each line also names any failed-run `MEASURE_HOME` of that path.
- **Clock trust.** ADR 0140's Trust line states that the notary host's clock is trusted for canary freshness and for the certificate time `attest check` uses (`connection_info.time`). No new check.
- **The authorised identity.** ADR 0140's Trust line records that the customer authorises the identity the probe prints with `--print-task`, which is accepted because the developer holds both roles.
- **The operator's transport public key.** `OPERATOR_TRANSPORT_PUB` is the output of `target/release/attest link-key ~/.config/hyalos-spike/operator-transport.key`, run from the usual `HOME` before step C. Setup first checks the file with `test -s` and refuses if it is missing or empty, rather than letting `link-key` create a key. The probe's notary start takes that hex as given.
- **Re-runs.** Each re-run after a failed run uses a fresh `MEASURE_HOME` and a fresh working directory, so it gets a fresh ledger key and chain. The failed run's `HOME` is kept with its failed-runs row until step D, which deletes it, and that path's step D line names it.
- **The variable refusal and the ledger.** The operator-side refusal of `HYALOS_TEST_*` and `HYALOS_TRUST_*` variables happens before `live` opens or creates the ledger. Slice 10's 10.3 test asserts that after the refusal no `ledger` directory and no ledger key file exist.
- **The copied binaries.** The probe prints the SHA-256 of each binary it copies to `SSH_HOST` and the commit it was built from. ADR 0140's Trust line records them.

## Consequences

Step D leaves nothing of the measurement on either machine, whether or not the measurement finished, and Slice 11 cannot be `done` until both lines exist. The Trust line names every trust the measurement rests on, including the two the developer accepts by holding both roles. A re-run never shares a ledger key or chain with the failed run it replaces.
