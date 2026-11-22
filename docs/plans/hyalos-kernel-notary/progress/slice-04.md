# Slice 4 — The audit needs only public keys and checks the public log's proofs

| | |
|---|---|
| **Status** | done |
| **Issue** | #389 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-21 |

## Preflight

Checked at `01c3e71`. Line numbers have moved since the plan was written, so each citation was found again by symbol.

| Citation | Holds |
|---|---|
| The ledger's `openssl` signing (`ledger.rs:25`), the key made on open (`:35`), `public_key` and `logged` (`:45`, `:95`) | holds |
| The audit's chain rule (`ledger.rs:122`) and run count (`:151`) | holds, moved |
| Where the log's reply is kept (`ledger.rs:55`) | holds |
| The audit's wiring (`main.rs:595`) | holds, moved |
| The pinned key at `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | holds. Its SHA-256 is `c0d23d6a…801d`, every stored reply's `logID` |
| Every check in the Contracts table, on the stored replies | holds for all six replies in `ledger/`, the plan's four and the two Slice 3 added |

Fields of a stored reply: `body` (base64, holds the hash, signature and public key), `integratedTime`, `logID`, `logIndex`, `verification.signedEntryTimestamp`, `verification.inclusionProof` (`checkpoint`, `hashes`, `logIndex`, `rootHash`, `treeSize`).

## Acceptance

From `spikes/task-kernel`: the monitor's tests passed 40 of 40, the attestation crate's 48 of 48, and `attacks` printed 51 `PASS` and no `FAIL`.

| ID | Shown by |
|---|---|
| S-04.1, S-04.1.e1, S-04.1.e2, 4.1 | `s_04_1_e1_e2_t_4_1_the_audit_completes_with_an_empty_home_and_two_public_keys`, `s_04_1_a_missing_public_key_is_refused_and_named`, `t_4_1_a_private_key_given_as_the_operator_key_is_refused`, `t_4_1_the_audit_does_not_open_the_kernel_file` |
| S-04.2, S-04.2.e1 | `s_04_2_e1_the_stored_entries_verify` |
| S-04.2.e2 | `s_04_2_e2_an_altered_inclusion_proof_is_refused_naming_the_link` |
| S-04.2.e3 | `s_04_2_e3_a_link_signed_with_a_test_key_is_refused_naming_the_link` |
| S-04.5.e1 | `s_04_5_e1_a_passing_report_lists_what_was_proven_then_the_five_limits` |
| S-04.5.e2 | `s_04_5_e2_a_failing_audit_names_the_first_check_and_prints_no_proof` |
| S-04.5.e3 | `s_04_5_e3_the_report_counts_three_runs_of_one_task` |
| 4.2 | `t_4_2_the_proofs_need_no_network_and_a_log_that_cannot_be_asked_refuses_the_audit`, and the run with no network below |

Also: `t_4_the_pinned_key_is_the_one_in_the_trusted_root_and_hashes_to_the_log_id`, `t_4_a_reply_for_another_link_is_refused`, `t_4_an_altered_signed_time_is_refused`, `t_4_an_entry_the_chain_does_not_show_is_refused`, `t_4_with_attestations_checked_the_report_has_no_not_checked_line_and_five_limits`, and the forgeries `t_4_a_discarded_result_with_its_intent_respelled_is_refused`, `t_4_a_link_of_another_kind_is_refused`, `t_4_a_second_result_for_one_run_is_refused_whichever_record_is_shown`, `t_4_one_record_claimed_by_two_runs_is_refused`, `t_4_a_result_naming_a_link_that_is_not_an_intent_is_refused`, `t_4_an_intent_labelled_with_another_task_or_guest_is_refused`, `t_4_a_run_relabelled_as_another_task_still_counts_in_the_total`, `t_4_a_record_whose_result_link_says_refused_is_refused`, `t_4_a_field_the_commitment_does_not_cover_is_refused`.

### What the Verify `grep` prints

Each line is in the code that writes links. The `audit` command builds an `Auditor`, which has no key path and never calls these.

| Line | Reached by |
|---|---|
| `ledger.rs:43` (the key's path), `:47` (`genpkey`) | `live` or `forge` → `Ledger::open` |
| `ledger.rs:55` (`public_key`) | `live` or `forge` → `Ledger::append` → `public_key` |
| `ledger.rs:73` (`self.key`) | `live` or `forge` → `Ledger::append` |

### The real audit

With an empty home directory, public keys only, and the real public log:

```
audit passed
proven: every link's signature verifies under the operator's public key
proven: every entry's inclusion proof and signed time verify under the public log's key
proven: the chain is unbroken and is the same list as the public log's entries under the operator's key
proven: this record is at link 1 of 6, with one result for its run
the task was run 2 times, as the intent links name the task
the chain holds 3 run(s) of any task under this key
not checked: the answers' attestations
not proven: the notary's independence is the customer's own
not proven: the model's name is the provider's claim
not proven: the answer's correctness is not judged
not proven: the log is trusted to have shown every entry under the operator's key
not proven: the request's hidden bytes are not shown, so the call may have used another credential or carried an added header
```

The home directory was still empty afterwards. In a network namespace with no network, the same command was refused: `the public log could not be asked which entries it holds under the operator's key`.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| The stored replies are four | There are six | Slice 3's real run added two. All six pass. The tests use the first four, as the plan says | n |
| Tests go in a module beside the code in `ledger.rs` | The module is declared there, and its text is in `kernel/src/audit_tests.rs`. The tests that run the command are in `kernel/tests/audit_cli.rs` | It keeps the ledger file readable, and only an integration test is sure to run the current binary | n |

## Decisions

- **The audit's command line:** `audit <record> [ledger dir] --operator-public <file> --notary-public <file>`. The operator's key is a PEM public key. A file holding anything else, a private key included, is refused.
- **The notary's key:** it must name a file. Nothing reads its contents until Slice 5.
- **The stand-in log:** `HYALOS_TRUST_LOG_DIR` and `HYALOS_TRUST_LOG_KEY`, both or neither. Its report prints `test trust settings` and does not use the words "public log".
- **Temporary files:** a new private directory `/tmp/hyalos-audit-<random>`, removed when the check ends. It is `/tmp` rather than `TMPDIR`, which can point under a home directory.
- **The shape of the chain:** every link is an intent or a result, every result names one earlier intent by its exact hash, each intent has at most one result, and exactly one result carries the record's commitment. That result must say `ok`.
- **The record binds its run:** the audit recomputes the task and guest hash from the record and refuses an intent that names others.
- **The run count:** a discarded run has no record to recompute, so its intent's task is the operator's label. The report says the per-task count is "as the intent links name the task", and also prints the total runs under the key, which a relabelled run cannot leave.
- **Order of checks:** every entry's proofs and signature, then the chain rule, then the log's list, then the record. Attestations run after these, and nothing is printed until all have passed.

## Discoveries

- **For Slice 5:** `audit_report` takes the attestation step's text, so the not-checked line goes away on its own when the step runs. `--notary-public` is already required, so Slice 5 only has to read it.
- **For the developer:** the per-task count of S-04.5 can be lowered by an operator who labels a discarded run with another task. The total under the key cannot. S-04.5 words the count "as the intent link names the task", so this holds the contract, but it is weaker than it reads.
- **For the operator:** the audit no longer derives the operator's key. A reader needs the operator's public key as a file, published out of band. Taking it from a log entry would trust whoever wrote that entry.
