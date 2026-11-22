# Slice 2 — The notary is its own program

| | |
|---|---|
| **Status** | done |
| **Issue** | #387 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-21 |

## Preflight

Checked at `62a05ea`. `context.md` cites line numbers from `3ef845b`. Slice 1 and its three follow-ups moved them, so each was found again by symbol.

| Citation | Holds |
|---|---|
| `spikes/task-kernel` is on `main` | holds |
| `attest` is its own workspace, on TLSNotary tag `v0.1.0-beta.1` | holds |
| `notary_key`, owner-only key file (`main.rs:155`) | holds, moved |
| `notary`, the session (`main.rs:175`) | holds, moved |
| In-memory pipe and spawned notary in `ask` (`main.rs:229`) | holds, moved |
| `request`, `body`, `verify`, size limits | holds, moved. Slice 1 added the binding |
| `ask`, `verify`, `notary` commands | holds, moved |
| Prover can reveal the server's identity (`ProveConfigBuilder::server_identity`) | holds in the pinned library |
| Verifier output carries the verified name (`VerifierOutput.server_name`) | holds in the pinned library |
| The protocol runs over any async stream (`Session::new`) | holds in the pinned library |
| `live.rs` calls `attest ask` and treats a failure as `ModelUnavailable` | holds |

## Acceptance

Run from `spikes/task-kernel` with `cargo test --release --manifest-path attest/Cargo.toml`: 48 passed, 0 failed, across four test binaries (9, 2, 10, 27). No test contacts the real provider or uses a real key or credential.

| ID | Shown by |
|---|---|
| S-01.1 | The three rows below |
| S-01.1.e1 | `s_01_1_e1_no_key_makes_an_owner_only_key_prints_it_and_listens` |
| S-01.1.e2 | `s_01_1_e2_a_key_from_an_earlier_start_is_kept` |
| S-01.1.e3 | `s_01_1_e3_a_key_others_can_read_refuses_the_start` |
| S-01.2, S-01.2.e1 | `s_01_2_e1_s_01_5_e1_a_session_is_attested_under_the_printed_key_with_one_line` |
| S-01.3 | The rows below, and `s_01_3_a_session_with_a_server_the_notary_does_not_approve_signs_nothing`, `s_01_3_a_certificate_under_a_root_the_notary_does_not_trust_is_a_provider_refusal` |
| S-01.3.e1 | `s_01_3_e1_a_certificate_for_another_name_is_refused` |
| S-01.3.e2 | `s_01_3_e2_s_01_5_e2_a_dropped_session_signs_nothing_and_the_next_succeeds`, `s_01_3_e2_the_notary_serves_the_next_session_after_a_drop`, `s_01_3_e2_a_link_cut_at_any_point_is_never_logged_as_a_refused_server` |
| S-01.4, S-01.4.e1 | `s_01_4_e1_t_2_3_the_operators_program_has_no_notary_key_and_the_layout_holds`, and the Verify `grep`, which printed nothing and exited 1 |
| S-01.4.e2 | `s_01_4_e2_with_no_notary_a_call_is_refused_and_a_key_file_there_is_not_opened` |
| S-01.5, S-01.5.e1 | `s_01_2_e1_s_01_5_e1_…` |
| S-01.5.e2 | `s_01_3_e2_s_01_5_e2_…` |
| S-03.5 (notary side), S-03.5.e3 | `s_03_5_e3_sequence_numbers_survive_a_restart_and_each_run_starts_at_zero`, `s_03_5_a_number_that_cannot_be_saved_is_not_signed`, `s_03_5_a_lost_sequence_file_refuses_the_start_rather_than_reuse_numbers`, `s_03_5_a_second_notary_on_one_directory_refuses_to_start`, `s_03_5_unreadable_run_sequences_refuse_the_start` |
| 2.1 | `t_2_1_an_unenrolled_transport_key_is_closed_before_the_protocol_starts` |
| 2.2 | `t_2_2_the_stand_in_replies_chunked`, `t_2_2_an_error_status_is_a_provider_refusal_and_spends_the_number`, `t_2_2_a_quiet_provider_ends_at_the_deadline_on_both_sides`, and `--present`, `--drop` in the S-01.3 tests |
| 2.3 | `s_01_4_e1_t_2_3_…`, `t_2_3_the_release_names_the_pinned_library_and_this_crate`, and the Verify `ls` |

The link's other rules have tests of their own: `t_2_1_a_notary_with_another_transport_key_is_refused_by_ask`, `t_2_1_an_attestation_under_another_signing_key_is_refused_by_ask`, `t_2_1_idle_strangers_cannot_keep_the_enrolled_operator_out`, `t_link_another_release_is_answered_release_and_closed`, `t_link_a_second_connection_during_a_session_is_refused_busy`, `t_link_a_session_above_the_size_limits_is_refused`.

S-03.5.e1, e2, e4 and e5 are the audit's checks, which Slice 5 adds.

**Not run:** a session through the separate notary against the real provider. This slice's criteria are all shown against the stand-in, and Slice 3's run is the first that needs the real one.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| The `provider` row of "How ask says why it refused" lists the notary's line as `server`, `limits`, `sequence` or `attested` | When the provider fails before answering, ask's word is `provider` and the notary's line is `dropped` | The notary cannot see why the operator's session with the provider stopped. S-01.3.e2 itself requires a refused line for a session dropped halfway | y: the developer accepted it on 2026-11-21 (ADR-013) |

## Decisions

- **Notary's arguments:** `--listen`, `--dir`, `--operator-transport-public`, `--approved-server`, `--root` (repeatable), `--stop-on-stdin-close`. The last is for tests, so a notary cannot outlive the test that started it.
- **Notary's files, all in `--dir`:** `signing.key` (32 bytes), `transport.key` (PKCS#8 DER), `sequences` (the header `hyalos.sequences/1`, one `<run> <next number>` per line, and `end <runs>`, so a file cut short is refused), `sessions.log`, and `lock`, which stops a second notary on one directory. The sequences are made before the signing key, and a key with no readable sequences refuses the start, so a lost file cannot restart a run at 0.
- **Session line:** `<unix seconds> <server or none> <sent> <received> attested <run> <sequence>`, or `… refused <reason>`.
- **Link libraries:** `tokio-rustls` (rustls with ring) for TLS 1.3, `rcgen` for the self-signed certificates. A public key is pinned as the hex of its DER `SubjectPublicKeyInfo`.
- **The operator's transport key:** made by `attest link-key <file>`, which writes it owner-only if absent and prints only the public half. Where it is kept is the operator's choice.
- **Ask's settings:** `HYALOS_NOTARY_ADDRESS`, `HYALOS_NOTARY_TRANSPORT_PUBLIC`, `HYALOS_NOTARY_SIGNER_PUBLIC`, `HYALOS_OPERATOR_TRANSPORT_KEY`, read from the environment so a monitor's child inherits them. None reuses `HYALOS_NOTARY`.
- **Ask's refusal word** goes to standard output, the stream the monitor reads. Refusals before a session starts (a missing setting, a credential of the wrong kind) stay on standard error and print no word.
- **Release identifier:** `build.rs` reads the tag from `Cargo.toml`, and the crate's version comes from Cargo.
- **Stand-in's controls:** `--present <name>`, `--reply plain|chunked`, `--status <code>`, `--quiet`, `--drop`.
- **An error reply is attested.** Ask reads the label only after the attestation, so a provider's error spends that run's number, as the `provider` row's `attested` says.
- **Handshakes:** the one-session slot is taken after the handshake, so a stranger cannot hold it. A handshake has 5 seconds. At most 16 run at once, and a new one past that closes the oldest.
- **The last two messages:** the attestation request and the attestation each end when the sender closes its half of the link, as they did in the spike, and each is bounded at 1 MiB. A notary that refuses closes with no attestation.
- **Deadline:** each side holds 30 seconds. The operator's clock starts first, so a link that drops within the notary's last second is logged `deadline`.

## Discoveries

- **For Slice 3:** until the monitor passes the four settings above, `live` with `HYALOS_ATTEST` fails closed with `ModelUnavailable`, because ask refuses without a notary. The monitor should map ask's word from standard output.
- **For Slice 3:** the first real-provider run through this notary also tests `server_identity` against the provider's real certificate chain, which no stand-in can show.
- **For Slice 6:** a stranger's refused connections each add a session line, so `sessions.log` can be grown by anyone who reaches the port. The contract asks for one line per session and sets no bound.
- Presentations made before this slice no longer pass, because they were signed by the old in-process key, which nothing reads now.
