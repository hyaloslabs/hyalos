# Slice 5 — The audit ties every answer to its run, and forgeries with the operator's keys fail

| | |
|---|---|
| **Status** | done |
| **Issue** | #390 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked at `6ca93b4`. Line numbers have moved since the plan was written, so each citation was found again by symbol.

| Citation | Holds |
|---|---|
| The `HYALOS_NOTARY` gate in the `audit` command (`main.rs:602` in the plan) | holds, moved to `main.rs:740` |
| The `attested` command and its per-answer check (`main.rs:607`, `:500`) | holds, moved to `main.rs:518` |
| The forge command (`main.rs:612`) | holds, moved |
| The chain rule that finds the result link holding the commitment (`ledger.rs:122`) | holds, moved to `ledger.rs:390` |
| The log's reply kept beside each link (`ledger.rs:78`) | holds, moved to `ledger.rs:88` |
| The tool-failure reason (`live.rs:66`) | holds |
| Verify's output names from Slice 1: `run`, `sequence`, `intent_hash`, `position`, `time` | holds |
| The signed times Slice 4 verified | holds. `check_entry` now returns the time it verified |

## Acceptance

From `spikes/task-kernel`: both builds ended `Finished`. The monitor's tests passed 54 of 54 (48 unit, 6 CLI), the attestation crate's 48 of 48, and `attacks` with both kernels printed 51 `PASS` and no `FAIL`. `grep -n 'HYALOS_NOTARY' kernel/src/main.rs` printed nothing and exited 1.

| ID | Shown by |
|---|---|
| S-03.2, S-03.2.e1 | `s_03_2_e1_s_03_3_e1_s_03_5_e1_s_04_3_e1_an_honest_record_of_eight_passes_and_others_are_refused` |
| S-03.2.e2 | `s_03_2_e2_s_04_4_e2_a_real_presentation_from_another_run_is_refused` |
| S-03.2.e3 | The honest-record test: answer 5 reuses answer 2's presentation, refused as another position |
| S-03.2.e4 | `s_03_2_e4_an_attestation_whose_request_carries_no_binding_is_refused`, and verify's own refusal, `s_03_4_a_request_with_no_binding_or_a_loose_one_is_refused` |
| S-03.3, S-03.3.e1 to e3 | The honest-record test, with signed times from entries the stand-in log signed, and `t_5_the_window_is_strict_at_the_intent_and_inclusive_at_the_result` |
| S-03.5, S-03.5.e1 | The honest-record test: sequence numbers 0 to 7 |
| S-03.5.e2 | `s_03_5_e2_a_question_asked_twice_with_the_first_answer_discarded_is_refused` |
| S-03.5.e4 | `s_03_5_e4_the_first_question_asked_three_times_is_refused` |
| S-03.5.e5 | `s_03_5_e5_an_operator_who_tells_the_notary_another_run_is_refused` |
| S-04.3, S-04.3.e1, S-04.3.e2 | The honest-record test: eight valid presentations, then one answer naming none |
| S-04.4, S-04.4.e1 | `s_04_4_e1_an_answer_attested_by_the_forgers_own_notary_is_refused` |
| S-04.4.e2 | `s_03_2_e2_s_04_4_e2_a_real_presentation_from_another_run_is_refused` |
| S-04.4.e3 | `s_04_4_e3_an_attestation_obtained_before_the_intent_was_logged_is_refused` |
| S-04.5 (attestation side) | `s_04_5_e1_a_passing_report_lists_what_was_proven_then_the_five_limits` and the honest-record test |
| 5.1 | `t_5_1_no_notary_setting_replaces_the_key_and_no_verify_program_is_taken_from_the_environment`, `t_5_1_a_notary_key_that_is_not_a_signing_key_is_refused`, `t_5_1_the_monitor_names_no_notary_setting`, and the Verify `grep` |

Also: `s_04_1_the_whole_audit_passes_with_an_empty_home` (the real command, an empty home, a kernel path that does not exist), `t_5_a_record_made_before_this_pack_is_refused_at_its_attestations`, `t_5_the_first_call_waits_until_two_seconds_past_the_intents_signed_time`, `t_5_with_no_signed_time_or_one_far_ahead_no_call_is_made`.

**Not run:** a real run through the real provider with the two-second wait. It needs the operator's credential, so no executor can run it, and this slice's Verify does not ask for one. Slice 3's real record predates the wait and this audit may refuse it.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Tests of the whole audit in the CLI test file | The passing run moved to `forgery_tests.rs`, which still runs the real command | It needs a record with real presentations, which that rig builds. `audit_cli.rs` now shows that a record made before this pack is refused | n |
| `attest/probes/two_machines.sh` is not in the slice's Files | Its `attested` line was updated | The command now takes key and verify as options, and the probe would otherwise break | n |

## Decisions

- **The verify program's input.** `--verify <program>`, on both `audit` and `attested`. It runs with a cleared environment and only the three `HYALOS_TRUST_*` settings, so it reads no home directory.
- **The trusted notary key file.** One line: the signing key as verify takes it, 66 lowercase hex characters beginning `02` or `03`. Anything else is refused.
- **The test's variable.** `HYALOS_TEST_TOLD_RUN`, read only by ask, for the run it tells the notary.
- **`attested` has no ledger.** It requires every answer to name one run, and checks position and sequence. It prints that the run's links and window are not checked there.
- **The wait's bound.** A log time more than 10 seconds ahead of the monitor's clock, or no readable time, refuses the run with `LedgerFailed`, and no call is made.
- **A record with no answers is refused**, since it has no attestation to check.

## Discoveries

- **For Slice 6 and the operator:** the real end-to-end audit under this slice's rules has not been run. The operator's next real run, through `attest/probes/two_machines.sh`, is the first that can pass it.
- **For the developer:** the audit does not replay the guest. It proves each answer was witnessed for this run and position, not that the guest asked those questions. `replay` checks that separately, given the guest.
- **For the retro:** a re-ask of the last position that keeps the first answer leaves no trace in the record. It gains the operator nothing, since the kept answer is the one they would have had anyway.
