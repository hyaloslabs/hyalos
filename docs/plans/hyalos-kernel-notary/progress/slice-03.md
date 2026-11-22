# Slice 3 — A run goes through the customer's notary, or does not run

| | |
|---|---|
| **Status** | done |
| **Issue** | #388 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-21 |

## Preflight

Checked at `435ab67`. `context.md`'s line numbers have moved since `3ef845b`, so each citation was found again by symbol.

| Citation | Holds |
|---|---|
| `live`: intent link, run, result link (`main.rs:544`, `:574`, `:581`) | holds, moved |
| `Reason` (`main.rs:36`) | holds |
| The tool's call and `ModelUnavailable` (`live.rs:49`, `:66`) | holds, moved |
| The direct provider call (`live.rs:75`), the credential field and read (`live.rs:18`, `:31`), the file's name (`main.rs:548`) | holds, moved. All removed here |
| `HYALOS_NOTARY` names the audit's trusted key | holds, unchanged |
| Ask's words and the notary program (Slice 2) | holds |

## Acceptance

From `spikes/task-kernel`: the monitor's tests passed 15 of 15, the attestation crate's 48 of 48, and `attacks` printed 51 `PASS` and no `FAIL`. No test contacts the real provider or the public log.

| ID | Shown by |
|---|---|
| S-02.1, S-02.1.e1 | `s_02_1_e1_an_attested_answer_names_a_presentation_hash_that_matches_the_file`, and the real run below |
| S-02.2, S-02.2.e1 | `s_02_2_e1_nothing_listening_refuses_the_run_and_the_result_link_says_so` |
| S-02.2.e2 | `s_02_2_e2_a_notary_that_goes_quiet_is_refused_at_the_deadline` |
| S-02.3, S-02.3.e1 | `s_02_3_e1_an_attestation_under_another_key_is_refused` |
| S-02.4, S-02.4.e1 | `s_02_4_e1_a_notary_on_another_release_is_refused_at_the_start` |
| 3.1 | The operator's run below: 40 of 40 answers verify under that notary's key |
| 3.2 | The operator's run below: 7.42 s for one call, at a 2.9 ms round trip |
| 3.3 | `t_3_3_with_no_notary_configured_the_run_is_refused`, `t_3_3_with_no_notary_a_task_that_asks_nothing_does_not_run`, `t_3_3_no_monitor_source_file_names_the_provider_or_the_credential_file` |

Also: `t_3_a_provider_error_is_its_own_reason`, `t_3_a_word_outside_the_set_is_the_reason_for_a_failed_tool`, `t_3_an_answer_with_no_presentation_is_refused`, `t_3_a_tool_still_running_at_35_seconds_is_ended`, `t_3_a_descendant_holding_the_output_is_ended_at_the_limit`, `t_3_a_record_path_in_use_is_not_fresh`.

### The operator's run (2026-11-21)

The operator ran this from `spikes/task-kernel`, with the notary on a second machine reached through an SSH tunnel:

```
bash attest/probes/two_machines.sh hivrr-bot hyaloslabs/hyalos 388
```

```
notary signing public key: 03d8359ac67c5ca74553fc7e7242bd7dc2b009217d8947dd7bb4531c93d3c8028f
round trip: median of 20 connections 2.9 ms
one attested call: 7.42 s wall, 304596 KB peak (operator side)
committed: link 5 in the public log, Bitcoin timestamp requested: true
ok: 40 inputs, 40 inferences by claude-haiku-5-5, fuel 31894
sent: https://github.com/hyaloslabs/hyalos/issues/388#issuecomment-7074028784
attested: 40 of 40 model answers match what the notary saw
not proven: the request's hidden bytes are not shown, so the call may have used another credential or carried an added header
```

The executor then ran `attest verify` on all 40 presentations under the real trust settings, with no credential. Each names `api.anthropic.com`, the run `3c687c7c…8c65` (the SHA-256 of `ledger/00004.json`), and a sequence number equal to its position, 0 to 39. The record, its salt and its presentations are in `spikes/task-kernel/out/two-machines/`.

The run was made at `e9ef572`. The review fixes after it change how a refused run ends and where the record is written, not the attested call.

The stop conditions did not trigger: the real run passed where the stand-in passed, and 7.42 s is under the 10-second target.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| The two-machine commands go into the progress file | They are one script, `attest/probes/two_machines.sh`, named above | A long pasted command wraps and breaks | n |
| The round trip is measured between the machines | It is measured to the second machine's SSH port, and the session itself crosses an SSH tunnel | The notary's port stays closed to the network. The tunnel's cost is inside the 7.42 s | n |

## Decisions

- **The monitor's settings:** `HYALOS_ATTEST`, `HYALOS_NOTARY_ADDRESS`, `HYALOS_NOTARY_TRANSPORT_PUBLIC`, `HYALOS_NOTARY_SIGNER_PUBLIC`, `HYALOS_OPERATOR_TRANSPORT_KEY`. A run with any of them missing logs its intent and a refused result, and its guest never starts.
- **The monitor's reasons:** `NotaryUnreachable`, `NotaryDeadline`, `WrongNotaryKey`, `ReleaseMismatch`, `ProviderRefused`. No notary configured is `NotaryUnreachable`.
- **A testable run:** `declared_run` holds the intent link, the execution, the result link and the record, and takes the link writer as a function, so tests never post to the public log.
- **An answer needs its presentation:** the monitor refuses an answer unless the hash ask printed is the hash of the file it wrote.
- **A record path is used once:** a live run refuses a path that already holds a record, salt or attestation directory.
- **A peer on another release:** `standin other-release` answers `release` on a real link, since a release cannot be set by configuration.

## Discoveries

- **For Slice 5:** `attested` does not yet check the run or the sequence number. All 40 hold in this run, by the separate check above.
- **For Slice 6:** one call took 7.42 s across the machines, against 1.88 s with both sides in one process. It is one measurement, through a tunnel.
- **For the operator:** the notary's keys and run sequences are on the second machine in `~/.hyalos-notary/state`. The operator's transport key is in `~/.config/hyalos-spike/operator-transport.key`.
