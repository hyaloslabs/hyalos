# Slice 3 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #429 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/429`, forked from `c21b54d`, before any edit.

| Citation | Holds |
|---|---|
| `kernel/src/main.rs`: `CURRENT_VERSION`, `read_transcript`, `witnessed`, `audit_report` | Yes |
| `kernel/src/ledger.rs`: the link version check and the operator-key check on `v == 2` | Yes |
| `attest/src/bin/forging-notary.rs`: `forged_ticket` | Yes |
| `kernel/src/forgery_tests.rs`: `World` and the `record_s_*` tests | Yes |
| `out/two-machines-record/record.json` is version 2 | Yes |

## Acceptance

Each row is a test of that name in `kernel/src/forgery_tests.rs`, unless it says otherwise.

| ID | Shown by |
|---|---|
| S-03.1, S-03.1.e1 to e7 | `canary_s_03_1_e1_*` to `canary_s_03_1_e7_*` |
| S-03.2, S-03.2.e1, e2 | `canary_s_03_2_e1_*`, `canary_s_03_2_e2_*` |
| S-03.3.e1 | The Verify audit of the committed record, which printed `audit passed` and `not proven: this record has no provider canary`, and `canary_s_03_3_e1_*` |
| S-03.3.e2, e3 | `canary_s_03_3_e2_*`, `canary_s_03_3_e3_*`, and `canary_a_version_1_record_cannot_carry_a_canary` |
| 3.1 | `canary_3_1_the_monitors_window_is_the_notarys` |
| 3.2 | `canary_3_2_provider_proofs_are_checked_on_version_3` |
| 3.3 | `canary_3_3_a_version_3_intent_naming_another_key_is_refused` in `kernel/src/audit_tests.rs` |

Verify results: every build finished. The attest suites passed (12, 2, 3, 41, 10, 23, 4 and 40). The monitor's tests passed (137 and 7) after the fix below, which changed only the monitor, so the attest suites were not run again. `out/live3.json` printed `replay identical`. The test-name `comm` printed nothing. The audit of the committed record reached the public log and passed. No test contacted a real provider, and no version 3 record has been made against the real provider.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `HYALOS_FORGE_CANARY` takes `missing`, `doubled`, `malformed` and `at=<unix seconds>` | It also takes `before=<seconds>` | S-03.1.e6 and e7 sit exactly on the window around a session time that a test cannot know in advance. The value is test-only and exists only in `forging-notary` | n |
| Files to modify did not list `kernel/src/live_tests.rs` | It changed | Its version 2 helper now names version 2, because new runs write version 3 | n |

## Decisions

- Monitor worlds for version 1 and 2 records use `forging-notary` with `HYALOS_FORGE_CANARY=missing`, because a real notary now always signs a canary and those versions must refuse one.
- A doubled or wrong-length canary is refused by `attest verify`, so the audit reports `the attestation does not verify`. A missing or stale one is named by the audit itself.
- The canary line comes straight after the attempt line, and the canary's limit is the last line, so the existing limits keep their positions.

## Discoveries

- For Slice 4: `execute_as` has its own version match, which the plan's suggested path did not name. The first run of the monitor's tests caught it.
- For Slice 4: `two_machines.sh` will now stop at the notary without a fresh pass until its gate is added.
