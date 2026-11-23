# Slice 2 progress

- **Status:** done
- **Issue:** #444
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `e86d262`. Citations in `context.md` use the paths from before Slice 1, so they were read under `hyalos-os/`.

| Citation | Result |
|---|---|
| `monitor/src/main.rs` limits, `wasmi` settings, guest run, `execute_as`, `replay`, `read_transcript`, `task_v2`, `task` command | Holds |
| `kernel-image/src/main.rs:168–181`, the kernel image's `wasmi` settings | Holds, and they equal the process backend's |
| `monitor/src/ledger.rs:387`, `:436`, the link version checks | Holds |
| `wasmi` 2.0.2 in both lockfiles | Holds |
| `.github/workflows/ci.yml:143`, the runner's KVM | Holds |

Version gates changed: `CURRENT_VERSION`, the task arm of `execute_as`, `replay`, `read_transcript` (both matches), `claim_of`, the intent in `declared_run`, the `task` command, and `ledger.rs` (known link versions, and intents that must name the operator).

Version gates read and left alone, because they already carry version 3's rule to version 4: the canary check in `witnessed` (`version < 3`), the ticket checks (`version != 1`), the replay and ticket checks in `audit_record`, and the conditions in `audit_report`.

## Acceptance

Tests are in `monitor/src/profile_tests.rs` unless named.

| ID | How it was shown |
|---|---|
| S-02.1, S-02.1.e1 | `s_02_1_e1_both_backends_map_one_profile_the_same_way`, and `profile/src/lib.rs`'s `s_02_1_profile_1_holds_the_settings_both_backends_used` |
| S-02.1.e2 | `s_02_1_e2_a_float_guest_is_refused_on_either_backend`, run on the process backend and on KVM |
| S-02.2, S-02.2.e1 | `s_02_2_e1_a_new_run_is_version_4_and_names_profile_1`, and `forgery_tests.rs`'s `s_02_2_a_version_4_record_passes_the_whole_audit_with_its_canary` |
| S-02.2.e2 | `s_02_2_e2_two_profile_names_give_two_task_identities`, and `s_02_2_the_version_4_identity_is_the_documented_frame` |
| S-02.3, S-02.3.e1 | `s_02_3_e1_every_committed_older_record_replays_byte_identically`. All 260 evidence files hash the same before and after |
| S-02.3.e2 | `s_02_3_e2_a_version_3_record_holding_a_profile_is_refused`, and `s_02_3_e2_a_profile_field_holding_null_is_refused_on_any_version` |
| S-02.4, S-02.4.e1 | `s_02_4_e1_an_unknown_profile_is_refused_and_named` |
| S-02.4.e2 | `s_02_4_e2_a_version_4_record_with_no_profile_is_refused` |
| S-02.5, S-02.5.e1 | `s_02_5_e1_both_backends_write_byte_identical_records`, run locally on KVM with `HYALOS_REQUIRE_KVM=1` |
| S-02.5.e2 | `s_02_5_e2_no_kvm_reports_skipped_unless_it_is_required` |
| S-02.6, S-02.6.e1 | `forgery_tests.rs`'s `s_02_6_e1_a_version_4_record_relabelled_version_3_is_refused` |
| S-02.6.e2 | `forgery_tests.rs`'s `s_02_6_e2_a_version_4_record_with_its_profile_changed_is_refused` |
| 2.1 | `monitor/tests/audit_cli.rs`'s `local_2_1_task_prints_the_version_4_identity`, which frames the identity itself |
| 2.2 | `local_2_2_the_kernel_images_wasmi_is_the_workspaces_and_the_profiles` |
| 2.3 | The `hyalos-os` job sets `HYALOS_REQUIRE_KVM`, opens KVM and adds the kernel image's target. `CLAUDE.md` lists `_KVM` |

Locally: 305 `hyalos-os` tests and 1,018 root tests passed, none failed. `cargo fmt`, clippy, `cargo xtask layout`, `lines`, `docs` and both `cargo deny` audits pass.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-02.6.e2: a changed profile is refused by a task or commitment mismatch | It is refused earlier, as an unknown profile | Only one profile exists, so any changed name is unknown. The test also shows a task recomputed for the new name differs | n |
| Moved tests stay unchanged | `record_s_01_1_e1` now uses version 5 as its unknown version, and `record_s_02_3_e1` expects links of `v: 4` | Version 4 is now known and is what new runs write | n |

## Decisions

- A refusal names a record's profile only when it is a plain profile name, since the field is the operator's text.
- The KVM comparison builds the kernel image from the tree each time, so it cannot compare against a stale image.

## Discoveries

- The kernel image's bytes change with this slice, as its code now reads the profile. Its behaviour is what S-02.5 compares.
