# ADR-030: Measurement failures, out-of-type probe replies and the `attest check` version

**Status:** accepted (developer, 2026-11-27); amended by ADR-035, a failure before any ticket is re-run under the same authorisation

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 4's panel

## Context

This amends ADR-024 and ADR-025. The review panel of the second cold-read run found three trust behaviours the #411 measurement (Slice 10) left open: what a failure before `live` does once a ticket is spent, what an out-of-type reply to a type probe does, and which request template `attest check` enforces, since it reads no record.

## Options

1. **Before `live`: re-run under the same authorisation.** Rejected: it needs spare attempts and blurs which sessions a passing run spent.
2. **Before `live`: a failed run, re-authorised.** Chosen.
3. **Out-of-type reply: record and continue to `live`.** Rejected: a provider that ignores a schema it was sent is a finding to stop on.
4. **Out-of-type reply: record and stop.** Chosen.
5. **`attest check`: root and notary only.** Rejected: the measurement would trust presentations whose template it never checked.
6. **`attest check --version <n>`.** Chosen.

## Decision

- A probe step that fails after a ticket is spent and before `live` is a failed run: the probe stops, the run is listed in the ceiling ADR's failed-runs table, and the developer re-authorises with `notary authorize --again` and runs the whole probe again. "Exactly 5 tickets spent" counts the passing round only.
- A type probe whose reply is outside its type is refused by `attest ask`. The probe records it in the probes table as `refused: out of type`, stops before `live`, and Slice 10 stops for the developer (risk R3).
- `attest check <presentation> --notary-public <key> --version <n>` rebuilds version `n`'s template and refuses a presentation whose request differs. The probes pass 5. A missing or unknown `--version` is refused, exit 1.

## Consequences

A provider that does not honour a version 5 schema halts the measurement before the ceiling is set. Every presentation the measurement trusts has had its root, notary and template checked.
