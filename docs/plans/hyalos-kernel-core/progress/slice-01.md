# Slice 1 progress

- **Status:** done
- **Issue:** #457
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked at base commit `2a26556`. Symbols were found by name, as `context.md` asks.

| Citation | Result |
|---|---|
| `hyalos-os/ABI.md`, the interface document (ADR-004 names its old `spikes/` path) | Holds |
| `hyalos-os/monitor/src/kvm.rs:181`, `answer` and its five requests | Holds |
| `hyalos-os/monitor/src/main.rs:150`, `hostile_kernels` and its expected reasons | Holds |
| `hyalos-os/monitor/src/main.rs:267`, the `refused:` string | Holds |
| `hyalos-os/monitor/src/profile_tests.rs:65`, the KVM gate | Holds |
| `hyalos-os/monitor/src/main.rs:327`, `HYALOS_KERNEL` chooses the backend | Differs by line only: `load_backend` is at `:319`, reading `HYALOS_KERNEL` at `:320`. Line 327 is now `main_inner`'s arguments |
| `hyalos-os/monitor/src/main.rs:325`, the audit opens no kernel file | Differs by line only: the comment is at `:317` |
| `hyalos-os/kernel-image/src/main.rs:6`, a `no_std` program, its own workspace (`hyalos-os/Cargo.toml` excludes it) | Holds |
| `hyalos.profile/1`, `hyalos_profile::PROFILE_1` | Holds |
| `design.md` §2 `hyalos.abi/1`: the four calls, their signatures, the limits 4,096, 65,536 with one effect, and 4,096 | Holds: `MAX_QUESTION`, `MAX_BODY`, `MAX_EFFECTS` and `MAX_RESULT` in `hyalos-os/audit/src/record.rs`, and `Host` in `hyalos-os/audit/src/replay.rs` |
| `.github/workflows/ci.yml:248`, the `hyalos-os` test step with `HYALOS_REQUIRE_KVM` | Holds |

No difference touches this slice's contracts.

## Acceptance

Tests are in `hyalos-os/monitor/src/abi_tests.rs`. Every case runs on the process backend and on the kernel image under KVM, and asserts the two give byte-identical transcripts or the same refusal. They ran locally on KVM with `HYALOS_REQUIRE_KVM=1`, so none could skip.

| ID | How it was shown |
|---|---|
| S-01.1 | `core_s_01_1_every_call_refuses_as_abi_md_states_on_both_backends`: 20 refusals across the four calls and the run, each checked against `ABI.md`'s refusal table, including the order of each call's checks, and each call answered at its limit |
| S-01.1.e1 | `core_s_01_1_e1_a_guest_calling_all_four_gives_byte_identical_transcripts`: `read` (length, `-1` for a missing and a negative index), `infer`, `effect` and `return` |
| S-01.1.e2 | `core_s_01_1_e2_read_into_too_small_a_buffer_returns_minus_2_on_both_backends`: caps 0 and 22 return `-2`, cap 23 returns 23 |
| S-01.2 | `core_s_01_2_e1` and `core_s_01_2_e2`, below. `ABI.md`'s `hyalos.abi/1` section is pinned by digest, the call names, signatures and limits are read from them and checked against both backends, and the kernel image's name is checked against the monitor's |
| S-01.2.e1 | `core_s_01_2_e1_the_unchanged_interface_passes` passes. It also loads each call with its documented signature, and refuses it with one more parameter or a changed result |
| S-01.2.e2 | `core_s_01_2_e2_a_limit_raised_without_a_version_change_fails`. Dry run: `MAX_QUESTION` raised to `8 * 1024` made it fail (`left: [("infer", 4096), …] right: [("infer", 8192), …]`); reverted. A second dry run changed `return`'s documented limit to 8,192 in `ABI.md`, and `core_s_01_2_e1` failed on the digest; reverted |
| S-01.3 | `core_s_01_3_anything_outside_the_interface_is_refused_alike`: other imports (WASI, another module, a memory, a global, a table), missing or mistyped `run` and `memory` exports, floats, SIMD, atomics, a start function and bytes that are not a module |
| S-01.3.e1 | `core_s_01_3_e1_a_guest_importing_hyalos_clock_is_refused_by_both_backends`: `BadImport` |
| S-01.3.e2 | `core_s_01_3_e2_a_guest_with_no_run_export_is_refused_by_both_backends`: `BadExport` |
| 1.1 | `core_1_1_abi_md_names_the_interface_and_states_every_call_limit_and_reason`: the section `hyalos.abi/1`, each call's signature and limit as `design.md` §2 gives them, `read`'s `-1` and `-2`, schema `0`'s labels, `read`'s `BadPointer`, and every `Reason` named in a table. `git grep -n 'hyalos.abi/1' -- hyalos-os/ABI.md` prints line 15 |

Verify, run locally on KVM: the `core_s_01` filter ran 8 tests, all passed. The whole `hyalos-os` workspace with `HYALOS_REQUIRE_KVM=1` and `--no-fail-fast` passed 375 tests, none failed or ignored, after building the `attest` programs as CI does. CI's `fmt` and `clippy` steps for the new crates, `cargo deny` on the `hyalos-os` workspace, `cargo xtask layout`, `lines` and `docs` pass.

No backend behaviour changed, so no committed guest's transcript changes. `s_02_5_e1` and `s_02_3_e1` still pass. `task-kernel run` of `guest/digest.wasm` on `evidence/fixtures/issues.tsv`, seeds `0` and `7`, built from the base commit and from this branch, gave the same transcripts on both backends: SHA-256 `24d971b2…46a5` for seed 0 and `0c7b1259…cbc2` for seed 7, all eight files. These base transcripts were kept outside the repository, as the shared contract asks.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Add a constant in the monitor (`hyalos-os/monitor/src/main.rs`) | The constant is `hyalos_audit::ABI`, in `hyalos-os/audit/src/record.rs` beside the limits it names. `main.rs` only adds the test module | The process backend's limits and `Host` live in `hyalos-audit`, which the monitor and `hyalos-verify` both use. A public constant there needs no unused-code allowance | n |
| A test asserts both constants equal the name in `ABI.md` | It does, and also pins the digest of `ABI.md`'s whole `hyalos.abi/1` section, prose and tables | Without the pin, a call could change along with its document and keep the name, which S-01.2 forbids | n |
| Files: create `abi_tests.rs`, modify `ABI.md` and `main.rs` | Also `profile_tests.rs` (`kvm_backend` made `pub(crate)`), `audit/src/record.rs` and `kernel-image/src/main.rs` | The shared contract asks every KVM test to use that gate, and the two constants live in those files | n |
| Verify's second command passes on its own | It needs the `attest` programs built first (`context.md` §4 lists the two `cargo build` commands, as CI's `hyalos-os` job runs them). Without them `attest/tests/notary.rs` fails with "forging-notary is not built" | The slice's Verify block leaves out the build steps | n |

## Decisions

- **Extra exports.** S-01.3 says an export "outside the interface" is refused. Both backends already refuse a missing or mistyped `run` or `memory`, and the suite tests that. A module that exports something besides those two is accepted by both, as it was before: the version-0 document only required the two, and refusing extras would change the interface, not fix it. The suite holds both backends to the same answer either way.
- **Where reasons are listed.** `ABI.md` splits reasons a call can produce (the pinned `hyalos.abi/1` tables) from those that come from a backend or a run's setting (`KernelFault`, `BadHypercall`, `Timeout`, `NoKvm`, `GuestMismatch`, the KVM backend's guest-size `TooLarge`). The second table is outside the pinned section, so a backend change there does not need a new interface version.
- **Order of refusals.** `ABI.md` states an order only for a call's argument checks, which `answer` and `Host` share. A live run's own reasons (`monitor/src/live.rs`) come after those checks in an order of the live source's, which the interface does not fix.
- **Kernel image constant.** It carries `#[allow(dead_code)]` and is checked from the source text, as `s_02_1_e1` checks `PROFILE`, because the kernel image is a separate workspace and does not use the name at run time.

## Discoveries

- **Guest size differs between backends.** The KVM backend refuses a guest over 1 MiB (`GUEST_MAX`, `hyalos-os/monitor/src/kvm.rs:242`) with `TooLarge` before it loads, and the process backend has no such limit. This is not a call's result, so it is not a stop under this slice, and `ABI.md` states it under "Not part of the interface". A later interface version, or the slice that splits out the runner, may want one limit for both.
- **Only KVM has a deadline.** `Timeout` cannot happen on the process backend. Also stated in `ABI.md`.
- **Verify needs the `attest` builds.** Later slices' Verify blocks that run the whole workspace need the two `cargo build` commands from `context.md` §4 first.
- **Pre-existing warning.** `monitor/src/live_tests.rs:441` ignores a `Result` (`unused_must_use`). Slice 9's lint work will meet it.
- **The kernel image file changes, its behaviour does not.** The image built from this branch differs from the base's at byte 41, inside the ELF header's section-table offset, so in sections that are not loaded. The `ABI` constant is unused at run time, and the transcripts above are identical.
- The suite runs in under two seconds on KVM, since it builds the kernel image once per test and each machine boots in milliseconds.
