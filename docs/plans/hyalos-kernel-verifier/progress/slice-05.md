# Slice 5 progress

- **Status:** done
- **Issue:** #447
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `657745c`.

| Citation | Result |
|---|---|
| `SERVER` at `attest/src/lib.rs:21` and `Trust::read` at line 78 | Holds |
| The check in `attest verify` | Holds, `verify` at `attest/src/main.rs:270` |
| Three `openssl` audit sites | Holds, moved by Slice 1 from `ledger.rs` to `audit/src/chain.rs` |
| 171 committed presentations | Holds |
| `tlsn-attestation` and `tlsn-core` build without `tlsn` | Holds, `cargo tree -p attest-check` has no `tlsn` |

## Acceptance

| ID | How it was shown |
|---|---|
| S-03.5.e1 | `s_03_5_e1_an_unreadable_home_is_not_needed_and_no_file_appears` in `verify/tests/alone.rs` |
| S-03.5.e2 | `s_03_5_e2_it_completes_where_it_cannot_write_a_byte`, see Deviations |
| S-04.1.e1 | `s_04_1_e1_it_passes_a_real_record_with_an_empty_path` |
| S-04.1.e2 | `s_04_1_e2_a_decoy_openssl_never_runs_and_a_forged_signature_is_refused` |
| S-04.2.e1, e2 | `s_04_2_e1_e2_both_checks_agree_on_every_stored_presentation` in `verify/tests/differential.rs`: 171 compared, 122 accepted by both including all 40 of `two-machines-canary`, 0 refused by both, 49 with no committed key compared on the key they name. One altered copy of each is refused by both |
| S-04.3.e1, e2 | `s_04_3_e1_e2_p256_and_openssl_agree_on_every_stored_signature`: 396 stored replies, 1,043 signatures accepted by both, 4 refused by both, 1,047 altered copies refused by both |
| S-04.4.e1, e2 | `s_04_4_e1_the_verify_option_is_rejected_as_unknown`, `s_04_4_e2_without_the_option_it_runs_normally` in `verify/tests/cli.rs` |
| 5.1 | `hyalos-os/attest-check` depends on `tlsn-attestation` and `tlsn-core`, holds trust by value, and reads no setting. `attest verify` is unchanged |
| 5.2 | `local_5_2_a_log_reply_over_its_limit_is_inconclusive`, `local_5_2_a_list_over_its_limit_is_inconclusive_before_any_entry_is_fetched`, `local_5_2_a_redirect_is_not_followed` |
| 5.3 | All three sites call `signature_verifies`, and S-04.3 covers each |
| 5.4 | `verify/tests/differential.rs` launches the built `attest verify` and compares its JSON key by key |
| 5.5 | `local_5_5_the_audit_no_longer_takes_a_verify_program` in `monitor/tests/audit_cli.rs` |
| 5.6 | `local_5_6_the_five_answer_record_verifies_inside_the_time_limit`: 3.8 ms on the `test-trust` build. Lines in `src/`: `hyalos-verify` 179, `hyalos-audit` 1,812, `attest-check` 365, total 2,356 |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-03.5.e2: no `/tmp` access | The test sets `RLIMIT_FSIZE` to 0, points `TMPDIR` at nothing, makes the working directory read-only, and scans the sources for `/tmp` and temporary-directory use | Removing `/tmp` for one process needs privileges a test runner may lack. The limit stops any byte being written but would not stop an empty file being created, which the source scan covers | n |
| Version 1 forgery tests read a report through an `old-verify` shim | Their notary signs no ticket | The audit launches no program, so a shim cannot stand in | n |

## Decisions

- A reply from the public log counts only if its status is 200. With redirects off, a 3xx arrives as a normal reply, and its body could otherwise read as an empty entry list.
- A panic while parsing a presentation is a refusal, and the audit commands silence panic text so it cannot echo what was being read.

## Discoveries

- For Slice 6: `attest/probes/two_machines.sh` no longer passes `--verify`. It was edited and not run, because it is operator-only.
