# Slice 4 progress

- **Status:** done
- **Issue:** #446
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `63a75ec`.

| Citation | Result |
|---|---|
| The corpus, `monitor/src/forgery_tests.rs` and `monitor/src/audit_tests.rs` | Holds |
| The ticketed path for a provider proof (`forgery_tests.rs:998` in the plan) | Holds, now `answer_with_a_proof` at line 966 |
| The report line `unchecked provider proofs: 1, which prove nothing` | Holds, now in `audit/src/report.rs` |
| The forging notary resolves from `hyalos-os/target/release` | Holds |
| The `hyalos-verify` command, `design.md` §2 | Holds |

## Acceptance

Tests are in `verify/tests/corpus.rs` unless named.

| ID | How it was shown |
|---|---|
| S-03.2 | `s_03_2_the_built_verifier_refuses_every_forgery_in_the_corpus`: 83 cases from 62 tests, each exit 1, with no pass line and the expected check. The test reports `ran 83 cases of 83 in the manifest` |
| S-03.2.e1 | `s_03_2_e1_a_record_whose_link_has_a_second_result_is_refused` |
| S-03.2.e2 | `s_03_2_e2_a_presentation_from_an_untrusted_notary_is_refused_naming_the_answer` |
| 4.1 | `local_4_1_the_manifest_names_each_test_outside_the_corpus_with_its_reason`, and `local_4_2_generate` fails unless the manifest's tests are the refusing tests |
| 4.2 | `monitor/src/fixture_tests.rs`'s `local_4_2_generate` |
| 4.3 | `local_4_3_only_public_keys_are_committed` |
| 4.4 | `local_4_4_an_unknown_provider_proof_keeps_its_report_line` |
| 4.5 | `local_4_5_the_five_answer_record_passes`, on `fixtures/pass/five-answers` |
| 4.6 | `local_4_6_the_verifier_does_not_depend_on_the_monitor`, and the slice's `cargo tree` command printed nothing |

Locally: 336 `hyalos-os` tests passed with `test-trust` and `HYALOS_REQUIRE_KVM=1`, none failed and none skipped. All committed evidence is unchanged.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Each corpus test becomes fixtures | The chain tests' helpers now commit to a real record under a salt of the result's label, and their links carry that label | Their commitments were placeholder strings, which no record file can match. Each still asserts the same refusal | n |
| The real-chain tests audit `live2.json` | Their fixtures use the whole committed chain and `two-machines/record.json`, with the same altered link | `live2.json` has no committed salt, so the verifier could not reach the chain check from it | n |
| `canary_a_version_1_record_cannot_carry_a_canary` reads through the `old-verify` shim | Its notary now signs no ticket | The built verifier reads real output, where the ticket would have been refused first and the canary check never met | n |

## Decisions

Five refusing tests are outside the corpus, each named with its reason in `manifest.json`, because none has a forged record for the verifier to read:

- `record_s_05_7_e4`: the notary refuses during a live run, before any record exists. This is the exclusion 4.1 names.
- `s_03_2_e4`: it calls the answer check directly on a verify report.
- `t_4_1`: it reads a key and no record, and a fixture would commit a private key.
- `record_s_05_8_e1` and `t_4_2`: a missing option and an unreachable log. Neither is a forged input, and the verifier is inconclusive (exit 2), shown by `local_3_3` and S-03.3.e1.

## Discoveries

- For Slice 5: the fixtures run `--verify` through `verify/tests/common/mod.rs`, so removing that option changes that one helper. Their expected checks must not change.
- For Slice 5: `fixtures/pass/five-answers` is the record to time for 5.6.
