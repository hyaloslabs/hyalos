# ADR-026: Key files, notary setup and attempts for the measurement

**Status:** accepted (developer, 2026-11-25); amended by ADR-028, free-text-ask replaces free-text-notary

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-025 and ADR-023. The final panel re-check found that the golden version 5 key set held only private halves while the verifier reads public files, that `free-text-notary`'s key had no source, that `notary public` can only read a key the notary has already made and the notary on the second machine predates Slice 10, and that the attempts the customer authorises do not cover the sessions the probe now opens before `live`.

## Options

1. **Derive the public halves at test time.** Rejected: the committed fixtures would not be what the verifier reads.
2. **Commit both halves.** Chosen.
3. **The probe copies and starts the notary itself, then pauses for the key.** Rejected: the operator's script would act as the customer.
4. **A stated customer step before each probe.** Chosen.
5. **Two authorisations, one for the checks and one for `live`.** Rejected: two ledger figures to reconcile.
6. **One authorisation per path.** Chosen.

## Decision

- `hyalos-os/audit/tests/golden/v5/` holds the private and public halves: the provider root and its key, `notary.key` and `notary.pub` (hex), `operator.key` and `operator.pub` (PEM), `log.key` and `log.pub` (PEM), and a stored-replies directory in place of a single captured file. `free-text-notary` signs with `golden/v5/notary.key` and builds its record against the same set.
- Before each probe, the customer (the developer) puts the Slice 10 notary on the notary host (copied to SSH_HOST for the two-machine path, local for one-process), starts it once on a named state directory so it makes its key, then runs `notary public --dir` there and hands the file to the probe. `one_process.sh` takes that state directory as an argument.
- The customer runs one `notary authorize` per path, with `--attempts` covering every session the probe opens, a number the plan states from the probe's step list. The ledger check counts tickets against that number.
- The environment check covers the operator's machine; the remote side is guarded by `attest check`. The probe's `test-trust` `task-kernel` builds into its own `--target-dir` outside `target/release`. A failed run prints every link and comment URL it has before it exits. A malformed or empty key file is refused at the probe's key step, exit 2. The up to 24-hour stale canary pass after an upgrade is accepted, and `CHANGES.md` advises the re-run.

## Consequences

Test-only public and private halves are committed under `golden/v5/`. The customer's setup becomes a stated step before each probe.
