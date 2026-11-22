# Slice 1 — The request carries the run's binding and the attestation carries the run's sequence number

| | |
|---|---|
| **Status** | done |
| **Issue** | #386 |
| **PR** | this PR |
| **Executor model** | Claude Fable 5.2 |
| **Date** | 2026-11-21 |

## Preflight

Checked against the branch's working tree, forked from `main` at `f531423`.

| Citation | Holds |
|---|---|
| `spikes/task-kernel` is on `main` | yes |
| `spikes/task-kernel/attest/src/main.rs:58` (`body`), `:78` (`request`), `:339` (exact-match check) | yes |
| `spikes/task-kernel/attest/src/main.rs:53` (size limits) | yes |
| `spikes/task-kernel/attest/src/main.rs:146` (`credential`), `:155` (`notary_key`) | yes |
| `spikes/task-kernel/attest/src/main.rs:175` (the notary's session), `:187` (verifier output), `:212` (session time), `:229` (in-memory pipe) | yes |
| `spikes/task-kernel/attest/src/main.rs:316` (`verify`), `:358` (commands), `:368` (failure reporting) | yes |
| `spikes/task-kernel/kernel/src/live.rs:49` (`infer`), `:59` (starts `ask`), `:66` (waits) | yes |
| `spikes/task-kernel/kernel/src/main.rs:61` (`Answer`), `:544` (`live`), `:574` (intent link) | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` (the pinned tag) | yes |
| The library's attestation builder takes extensions and a verified presentation returns them | yes: read in the pinned source, `crates/attestation/src/builder.rs` and `presentation.rs` |

No difference touched a contract.

## Acceptance

Tests are in `spikes/task-kernel/attest`. Unit tests sit in `src/main.rs`; the others are in `tests/binding.rs` and run the built programs against the stand-in provider. `cargo test --release --manifest-path attest/Cargo.toml` printed `test result: ok. 7 passed; 0 failed` for the unit tests and `test result: ok. 9 passed; 0 failed` for `tests/binding.rs`.

| ID | How it was shown |
|---|---|
| S-03.1 | By the operator against the real provider (1.1 below). Against the stand-in: `s_03_1_a_request_with_a_binding_is_answered_and_verify_reports_the_binding`, `s_03_1_the_default_trust_settings_refuse_the_stand_in_by_its_certificate`, `s_03_1_the_test_root_alone_refuses_the_stand_in_by_its_name` and `s_03_1_any_one_test_setting_marks_the_report` |
| S-03.4 | `s_03_4_a_request_with_no_binding_or_a_loose_one_is_refused` and `s_03_4_only_the_head_this_tool_writes_is_accepted`, with the two rows below |
| S-03.4.e1 | `s_03_4_e1_the_template_with_a_title_and_a_binding_is_accepted` |
| S-03.4.e2 | `s_03_4_e2_a_different_model_is_refused`, `s_03_4_e2_different_instructions_are_refused` |
| 1.1 | The operator ran one attested call with a binding against the real provider on 2026-11-21, with the command under Decisions. Ask printed label 0, model `claude-haiku-5-5`, response `msg_011ChH3q7CkUqnAHFMnQFRbT`, sequence 0. Verify of that presentation printed the same intent hash and position 0 that were sent, run equal to the intent hash, server `api.anthropic.com`, session time 1795308656, and no `trust` field |
| 1.2 | `t_1_2_verify_reports_the_run_and_sequence_the_notary_signed`, `t_1_2_an_altered_sequence_number_does_not_verify`, `t_1_2_an_altered_run_does_not_verify`, and the unit tests `t_1_2_the_run_and_sequence_number_read_back_as_written` and `t_1_2_the_in_process_notary_counts_each_run_from_zero` |
| 1.3 | `t_1_3_verify_reports_the_session_time_the_notary_recorded` |
| 1.4 | `target/release/task-kernel replay guest/digest.wasm out/live3.json` printed `replay identical` |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Suggested path step 2: try the binding with the operator before building the rest | The rest was built and tested against the stand-in first, and the operator's call came last | The call needs the built tool. The stop condition would have held either way | n |
| Test names carry the ID with dots and dashes as underscores | Local criteria are prefixed `t_`, so `1.2` is found by searching for `t_1_2` | A Rust test name cannot start with a digit | n |

## Decisions

- **The binding's form.** `metadata.user_id` holds the intent hash as 64 lowercase hex characters, a dot, and the position as a decimal number with no leading zeros. Verify accepts only that form.
- **The extension.** Identifier: the bytes of `hyalos.run-sequence/1`. Value: 72 bytes, the run as 64 lowercase hex characters in ASCII, then the sequence number as 8 big-endian bytes. Verify requires exactly one extension with that identifier. Slice 2 reuses both unchanged.
- **Ask's arguments.** `ask <presentation file> <intent hash> <position>`, with the question on standard input as before.
- **Verify's output names.** Added to the existing ones: `run`, `sequence`, `intent_hash`, `position`, and `trust` with the value `test trust settings` when any trust setting is not the default. Ask's line gained `sequence`.
- **Trust-setting variables.** `HYALOS_TRUST_SERVER_NAME`, `HYALOS_TRUST_ROOT` (a file holding one root certificate in DER, added to Mozilla's roots) and `HYALOS_TRUST_PROVIDER_ADDRESS`. They are read in `attest/src/main.rs` until Slice 2 moves them to the library file.
- **The stand-in provider.** `standin <root file> <server name>` writes its test root to the file, prints `listening <address>`, answers every request with one fixed typed reply (label 0), and exits when its standard input closes. It uses two new dependencies of the `attest` crate, `rcgen` and `tokio-rustls`.
- **What a request may hide.** Review of this slice's pull request found that verify checked too little of the request's head. Verify now requires the shown head to equal, byte for byte, the head this tool writes for the reported title and binding, and refuses a hidden run longer than 256 bytes. Ask refuses a credential longer than that.
- **Test settings and real credentials.** Under any test trust setting, ask refuses a credential that starts `sk-ant-`, so test settings cannot send a real credential elsewhere. Ask's line carries `trust` as verify's does, and the monitor's attestation check prints `test trust settings:` before its line when any answer's report carries it. A trust variable that is set but is not text is a refusal.
- **The operator's command for 1.1.** From `spikes/task-kernel`, with `A` the built `attest` program and `P` a new presentation path: the title `Two intermittent test failures can turn a release's CI red` on standard input to `A ask P 5f1d0c7a9b3e4f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8 0`, then `A verify P` with the key `A notary` prints.

## Discoveries

- **Presentations made before this slice no longer verify.** Their requests carry no binding, so the template check refuses them. That includes `spikes/task-kernel/out/live3.json.attest` and `out/q.presentation`. Records still replay. Slice 5 needs presentations made after this slice for its tests.
- **A whole attested session against the stand-in takes about half a second** on one machine, so tests can afford one per test file.
- **Ask still prints the in-process notary's public key** and reads the notary key. Slice 2 removes both.
- **Each `ask` is its own process,** so the in-process notary's sequence number is always 0 in a real run. A run with more than one question will only number them 0, 1, 2 once Slice 2's notary keeps the count.
- **A hidden run cannot be proven free of line breaks.** Verify sees only the hidden run's length. Within 256 bytes a cheating operator could still hide a second header after the credential, and a short body after that, if the provider were to honour it over the shown `content-length`. Whether the provider does was not tested. Closing this needs either a bound as tight as the real credential's own length or a way to show the hidden bytes hold no line break. It matters from Slice 2 on, when the notary is no longer the operator's, and is filed as #394 for the developer.
- **The provider accepted the binding** in `metadata.user_id` and answered as before. Nothing in its reply echoes the field.
