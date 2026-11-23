# Slice 3 progress

- **Status:** done
- **Issue:** #445
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `b3caebc`. Citations in `context.md` use the paths from before Slice 1, and line numbers moved in Slice 2.

| Citation | Result |
|---|---|
| The record types, host, `execute_as`, `replay`, `read_transcript`, `check_answer`, `witnessed`, `audit_inputs`, the stand-in log, `audit_record`, `audit_report` (`monitor/src/main.rs`) | Hold |
| `read_links`, key reading, inclusion proof, `Log`, `Audited`, `Claim`, `Auditor` (`monitor/src/ledger.rs`) | Hold |
| Key generation, link signing and log submission stay in the monitor | Holds |

## Golden reports

Captured from the monitor at `b3caebc`, committed in `cf50728` before any audit code moved. Each ran with a stand-in log of `evidence/ledger`, the guest `guest/digest.wasm` and the operator key in `evidence/out/two-machines-slice5/operator.pub`.

| Record | Version | Notary key from | Option |
|---|---|---|---|
| `evidence/out/two-machines/record.json` | 1 | `two-machines-slice5` | `--legacy-v1` |
| `evidence/out/two-machines-slice5/record.json` | 1 | `two-machines-slice5` | `--legacy-v1` |
| `evidence/out/two-machines-record/record.json` | 2 | its own directory | none |
| `evidence/out/two-machines-canary/record.json` | 3 | its own directory | none |

Not golden, because they pass under no committed key set: `c1.json`, `c2.json`, `fakeA.json`, `fakeD.json`, `live2.json` and `live3.json`. The other committed transcripts have no salt, so no audit can pass them.

## Acceptance

Tests are in `verify/tests/cli.rs` unless named.

| ID | How it was shown |
|---|---|
| S-03.1 | `s_03_1_every_committed_record_gives_its_golden_report` |
| S-03.1.e1 | `s_03_1_e1_the_canary_record_matches_the_monitors_audit` |
| S-03.1.e2 | `s_03_1_e2_a_version_1_record_without_the_option_is_refused_naming_it` |
| S-03.3, S-03.3.e1 | `s_03_3_e1_a_public_log_that_cannot_be_reached_is_inconclusive`, at the library's verdict |
| S-03.3.e2 | `s_03_3_e2_a_missing_key_file_is_inconclusive_naming_its_role` |
| S-03.4, S-03.4.e1 | `s_03_4_e1_the_dependency_tree_holds_no_task_running_code`, and the slice's `cargo tree` command printed nothing |
| S-03.4.e2 | `s_03_4_e2_the_source_has_no_live_kvm_or_signing_code` |
| S-03.6, S-03.6.e1 | `s_03_6_e1_a_presentation_of_exactly_the_limit_proceeds_to_the_check` |
| S-03.6.e2 | `s_03_6_e2_a_presentation_over_the_limit_is_refused_naming_it`, and `s_03_6_every_local_input_over_its_limit_is_refused_before_parsing` |
| S-03.7, S-03.7.e1 | `s_03_7_e1_a_release_build_ignores_every_stand_in_setting`, on a release build without `test-trust` |
| S-03.7.e2 | `monitor/src/forgery_tests.rs`'s `s_03_7_e2_a_release_build_refuses_a_stand_in_providers_record`, and `s_03_7_e2_provider_settings_never_reach_the_child_in_a_release_build`, which runs `audit/tests/no_stand_in.rs` without the feature |
| 3.1 | `monitor/tests/audit_cli.rs`'s `local_3_1_the_monitors_audit_gives_every_golden_report` and `local_3_1_the_monitors_links_directory_still_defaults_to_ledger`, and the monitor's existing tests |
| 3.2 | `local_3_2_no_release_selection_enables_test_trust`, and the slice's `cargo tree` command printed nothing |
| 3.3 | `local_3_3_every_audit_failure_is_refused_or_inconclusive`. An error with no stated kind is a refusal (`audit/src/failure.rs`) |
| 3.4 | `local_3_4_the_verify_program_is_required_until_slice_5`, and the unlaunchable rows of the 3.3 table |
| 3.5 | `LogSource::from_env` and `Verifier::from_env` read no setting without `test-trust`, shown by the S-03.7 tests |
| 3.6 | `audit/tests/golden/`, with each record's inputs listed above |
| 3.7 | `local_3_7_an_altered_chain_or_answer_is_refused` |

Locally: 326 `hyalos-os` tests passed with `test-trust` and `HYALOS_REQUIRE_KVM=1`, none failed and none skipped. All 260 evidence files are unchanged.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| A committed record uses the keys in its own directory | The version 2 and 3 records use their own notary key and the operator key in `two-machines-slice5` | Their directories hold no operator key. It is the key the chain's links name | n |
| Capture each transcript `audit_cli.rs` or `audit_tests.rs` audits to a pass, with a temporary patch | None was captured | Neither file audits a transcript to a pass. `audit_tests.rs` calls the chain check and the report separately | n |
| S-03.3.e1: the verifier runs against an unreachable log | Shown at the library's verdict, and the exit code for an inconclusive verdict is shown through the program | No setting can point the program at another log, by design | n |
| S-03.7.e2: exit 1 on a stand-in provider's presentation | The record is refused at the pinned log key, before its presentation is checked. A second test proves the provider settings do not reach the child | A stand-in's chain cannot pass the real log's key | n |

## Decisions

- The monitor's tests that use a stand-in log are compiled only with `test-trust`, so `cargo test` without the feature does not run them. CI and this slice's Verify command set it.
- CI now runs `fmt` and `clippy` on `hyalos-profile`, `hyalos-audit` and `hyalos-verify`.

## Discoveries

- The golden reports carry the stand-in log's wording, because they are captured offline. A check against the real log is Slice 7's.
