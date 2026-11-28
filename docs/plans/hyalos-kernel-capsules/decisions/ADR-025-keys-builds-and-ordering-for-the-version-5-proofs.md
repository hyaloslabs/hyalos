# ADR-025: Keys, builds and ordering for the version 5 proofs

**Status:** accepted (developer, 2026-11-25); amended by ADR-026, the key files, notary setup and attempts; amended by ADR-028, free-text-ask replaces free-text-notary; amended by ADR-030, failures before `live`, out-of-type probe replies and `attest check --version`; amended by ADR-034, concurrency from the notary's code and its busy test, no live busy check; amended by ADR-036, notary public's outcome for an unusable signing key

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-023. The panel's re-check found that the notary has no way to give its public key except its own standard output, that the golden version 5 record and the free-text test have no fixed key source, that the measured build and the order of the probe steps are open, and that a default build before the pin reads every version 5 record as inconclusive, so S-10.3.e2 and e3 cannot show a refusal in Slice 9. ADR-023's rejection of "a Cargo feature on `attest`" means a feature that changes `attest`'s own behaviour; a feature that only gates a separate test binary, as `forge-ticket` does, is allowed. ADR-023's pre-pin rule (inconclusive, exit 2) also amends ADR-012, ADR-016 and ADR-019 where they say a default build refuses version 5.

## Options

1. **The customer keeps the notary's printed key, or the requirement is dropped.** Rejected: the first rests on a terminal copy, the second keeps the key taken from the thing checked.
2. **A `notary public` subcommand.** Chosen.
3. **Public keys only, regenerated on a re-pin.** Rejected: a re-pin would replace every golden file.
4. **Commit the stand-in key set, private halves included.** Chosen: they are test keys trusted only by `test-trust` builds.
5. **Loosen 10.5 to "at least one" link and comment.** Rejected.
6. **A pinned build and checks before `live`.** Chosen.
7. **Prove S-10.3.e2 and e3 only after the pin.** Rejected: Slice 9 would hold no check on them.
8. **Both sides of the pin.** Chosen.

## Decision

- Slice 10 adds `notary public --dir <dir>`, which prints the public half of the notary's signing key from its directory. The customer runs it on the notary host and hands the file to the probe; the probe never reads the key from the notary's output.
- The golden version 5 record's stand-in key set (provider root and its private key, notary signing key, operator key, a stand-in log key and its captured log) is committed under `hyalos-os/audit/tests/golden/v5/`, test-only, so Slice 11 can re-pin. `free-text-notary` signs with its own stand-in key; the S-06.5.e2 test passes that key to the verifier and asserts the template rule's message as well as exit 1. The 10.6 test passes the stand-in session's own notary key and asserts a root refusal.
- Each probe script builds `task-kernel` itself with `--features test-trust` only and refuses to run when any `HYALOS_TEST_*` or `HYALOS_TRUST_*` variable is set. The real-root `attest check`, the busy-session check and the probes run before `live`. A failure after `live` stops; the ceiling ADR records every log link and comment, and the re-run uses a new issue.
- S-10.3.e2 and e3: Slice 9 expects inconclusive (exit 2) from a build without `test-trust`; Slice 11 re-runs `no_stand_in_v5` after the pin and expects refused (exit 1). Both refuse before any network call.

## Consequences

The notary grows one small subcommand. Test-only private keys enter the repository under `audit/tests/golden/v5/`. A failed live run costs a second public comment on a new issue.
