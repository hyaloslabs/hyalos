# ADR-024: The real-root presentation check and the small-limit deadline test

**Status:** accepted (developer, 2026-11-25); amended by ADR-030, failures before `live`, out-of-type probe replies and `attest check --version`

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-023. ADR-023 asks Slice 10 to check each presentation with a default-build `attest-check`, but `attest-check` is a library with no command, `attest verify` honours the stand-in trust variables, and a default-build `hyalos-verify` finds a pre-pin version 5 record inconclusive. Separately, the S-08.3.e2 integration test at L = 200 would run about 45 minutes.

## Options

1. **Record the unset variables only.** Rejected: rests on the developer's word.
2. **Check after the pin with `hyalos-verify`.** Rejected: the ceiling would be set before its evidence is checked.
3. **A new `check` subcommand on the attest tool.** Chosen.
4. **Keep the 200-question integration test in CI.** Rejected: about 45 minutes.
5. **Drop the integration test.** Rejected: the unit test's oracle is the formula itself.
6. **An integration test at a small limit.** Chosen.

## Decision

- Slice 10 adds `attest check <presentation> --notary-public <key>`, which runs `attest-check` on one presentation against the real provider roots only. It ignores every `HYALOS_TRUST_*` variable in every build; a test proves it refuses a stand-in-rooted presentation with the variables set. The measurement runs it on each presentation.
- The S-08.3.e2 integration test uses a contract with a limit of 5 questions and the stand-in delay at 1.4 × `CALL_SECONDS`, and must finish before its deadline. The `live_deadline` unit test covers L = 200.

## Consequences

The attest tool grows by a few dozen lines in Slice 10. The integration test runs in about a minute.
