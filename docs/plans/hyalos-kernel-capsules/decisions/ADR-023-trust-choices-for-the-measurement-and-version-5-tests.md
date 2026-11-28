# ADR-023: Trust choices for the #411 measurement and the version 5 tests

**Status:** accepted (developer, 2026-11-25); amended by ADR-024, the `attest check` command and the small-limit deadline test; amended by ADR-025, the notary key, test keys, the measured build and ordering, and the S-10.3 proof; amended by ADR-026, the key files, notary setup and attempts; amended by ADR-028, free-text-ask replaces free-text-notary

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-019 and ADR-010. The cold read's review panel found that no slice gives `live` a way to write version 5 before the pin, that a live run also writes a public log link and a GitHub comment and needs a customer authorisation and canary pass, that nothing proves the measured figures came from the real provider, that no test can get a free-text request signed, that the golden version 5 record has no fixed stand-in keys, and that the new audit failures name no exit code.

## Options

1. **Measure with `attest ask` alone.** Rejected by ADR-019.
2. **A new switch to skip the public log and GitHub effect.** Rejected: more test surface in the monitor for one measurement.
3. **A `HYALOS_TEST_*` variable or a Cargo feature on `attest` for the free-text request.** Rejected: the first ships in the release binary, the second ADR-014 rejected for the other hooks.
4. **Regenerate the golden record on each run.** Rejected: it is no longer a golden record.
5. **The choices below.** Chosen.

## Decision

- `live` gains `--version 5`, accepted only in a `test-trust` build and refused in a default build. Slice 10 adds it.
- For the measurement the developer acts as both customer and operator: authorises the task, runs the canary with their own credential, and accepts one public log link and one issue comment per network path, on a repository the developer owns. Slice 10 extends `hyalos-os/attest/probes/two_machines.sh` and adds `hyalos-os/attest/probes/one_process.sh`.
- One credential, `~/.config/hyalos-spike/anthropic.key`, serves both roles; it never enters the repository.
- The measurement records that every `HYALOS_TRUST_*` variable was unset, checks each presentation with a default-build `attest-check`, and takes the notary's public key from the customer side, not from the notary's standard output.
- A free-text request signed blind comes from a separate test-only binary, `free-text-notary`, built as `forging-notary` is, behind its own feature. The refusal is shown with a `test-trust` verifier on stand-in trust.
- The golden version 5 record verifies against a committed stand-in `root.der`, notary public key and operator public key beside `hyalos-os/audit/tests/golden/log.pub`.
- Exit codes: refused (1) for a contract that does not parse, answers over the limit, a mismatched answer type and a request that is not the version's template; inconclusive (2) for a version 5 record read by a build from before the pin.
- `hyalos-os/CHANGES.md` tells customers to re-run `canary check` after upgrading.
- Live-side failures (provider late or unreachable, the deadline, a busy notary, the public log, the comment, a reply over `MAX_RECV`) keep today's reasons, cited where each is set.
- The developer runs the KVM steps (Slice 1's `run.sh`, Slice 8's heap measurement) when the executor's host has no `/dev/kvm`, recorded in the progress file.

## Consequences

Slice 10 owns the `--version 5` option and the two probe scripts. Slice 6 owns `free-text-notary`. Slice 9 owns the committed stand-in keys. The measurement leaves public traces: one log link and one comment per path.
