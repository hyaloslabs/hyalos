# Slice 9 progress

- **Status:** done
- **Issue:** #465
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked on branch `issue/465`, forked from `origin/main` at `6620c98` (Slice 8 merged). Symbols were found by name.

| Citation | Result |
|---|---|
| Stop condition: #449 still open | Holds: #449 is closed |
| Slice 9 **Contracts**: `task-kernel fuzz`, `hyalos-os/monitor/src/kvm.rs:329`, with its fixed seed | Differs by line only: `kvm::fuzz` was at `:476` on the base and is at `:581` after formatting. Its seed (`0x9e37_79b9_7f4a_7c15`) and the `fuzz` command are unchanged |
| `context.md` §1: `kvm.rs:329–365`, "Fuzzes the request handler. Kept. Runs in CI" | Differs by line only, as above; it did not run in CI before this slice |
| Pattern: the root's lint table at `Cargo.toml:14` | Holds: `[workspace.lints.rust] unsafe_code = "forbid"` at `:13–14`, clippy `all` at warn below it |
| Pattern: crate-level `forbid` in `profile`, `audit` and `verify` | Holds, and `attest-check/src/lib.rs` has one too. All four are kept |
| `design.md` §2: the `unsafe` list, seven files at `63a75ec`, and `runner.rs` from Slice 7 | Differs: three test files the verifier pack added since also hold `unsafe` (`audit/tests/no_stand_in.rs`, `verify/tests/alone.rs`, `verify/tests/real.rs`, for `env::set_var` and `pre_exec`). They carry a file-level allow too; see Deviations. Not a contract difference: the list itself is Slice 10's |
| `design.md` **Modifier: infra**, CI's workspace step with `test-trust` features (`ci.yml:248`) | Differs by line only: `:250`, and it also enables `test-switches` (Slice 7) |
| The kernel image and the guest are separate workspaces with their own lockfiles | Holds: each has `[workspace]` and a `.cargo/config.toml`; the kernel image's sets its target, the guest's does not, so its clippy takes `--target` |
| Progress debt addressed to Slice 9: `live_tests.rs:441` `unused_must_use` (Slice 1, 8), `kvm.rs`'s `tick as usize` (Slice 8), the monitor, kernel image and `attest/tests/canary.rs` not formatted (Slices 5–8), lints with and without `test-switches` (Slice 8) | Holds: all present on the base, all fixed here |

No difference touches this slice's contracts.

## Acceptance

Run locally on KVM with Landlock ABI 3, `HYALOS_REQUIRE_KVM=1` and `HYALOS_REQUIRE_LANDLOCK=1`, so no KVM or Landlock test could skip.

| ID | How it was shown |
|---|---|
| S-05.3 | Every Verify command exits 0: `cargo fmt --check` on the workspace, the kernel image and the guest; `clippy -D warnings` on the workspace with `test-trust` and `test-switches`, with `test-trust` alone and with no feature, on `forging-notary` with `forge-ticket`, on the kernel image with and without `selftest`, and on the guest for `wasm32-unknown-unknown`. CI's `hyalos-os` job runs all of them in its new step **Format and lint every crate** |
| S-05.3.e1 | Structural: the above, on the branch as it will merge |
| S-05.3.e2 | Structural, counter-checked: a planted `fn planted(v: &Vec<u8>)` in `monitor/src/ledger.rs` fails the workspace clippy (`ptr_arg`, exit 101); the same in the kernel image fails its clippy; a planted `unsafe` block in `ledger.rs`, not on the list, fails with `usage of an unsafe block`; a mis-spaced function in the guest fails its `fmt --check`. Each was restored. The dry run on the pull request is recorded under **CI dry runs** below |
| S-05.5 | `task-kernel fuzz evidence/fixtures/issues.tsv 100000` prints `100000 random requests: 7075 answered, 92925 refused, no crash` and exits 0 in the release build (0.3 s). `core_s_05_5_frame_fuzzer` (`monitor/src/runner_tests.rs`): 100,000 hostile frames from the fixed `FRAME_FUZZ_SEED`, each on a socket pair that then closes; `read_frame` gives exactly the frame the channel's definition gives or refuses it as a fault, a frame decodes as a request, a reply or a `Boot` only when it is the one encoding of what it decodes to, and the generator reaches every request type, reply tag, the boot frame and a refusal (1.1 s). CI runs the first in its new step **Fuzz the request handler** and the second with the workspace's tests |
| S-05.5.e1 | Both fuzzers above finish their rounds on their fixed seeds with no panic or crash |
| S-05.5.e2 | Fuzz, counter-checked: a planted `assert!` in `kvm.rs`'s `answer` that panics on a large effect pointer makes `task-kernel fuzz` exit 101 (`planted`). For the frame fuzzer, a `Reply::decode` that ignores bytes after a limit fails it at frame 1359 (`a reply decoded from another encoding`), and an unchecked `fields[..4]` in `Request::decode` fails it with the decoder's panic. Each was restored. The dry run on the pull request is recorded under **CI dry runs** below |
| 9.1 | The formatting commit (`style(hyalos-os): format every hyalOS crate`) is `cargo fmt` output only. On it, the whole workspace passed on KVM (459 tests), including `core_s_02_1_e1_…`, which pins the base transcripts' SHA-256 for seeds 0 and 7. Differential dry run on the final branch: release builds of the base (`6620c98`) and this branch, `HYALOS_KERNEL=<image> task-kernel run guest/digest.wasm evidence/fixtures/issues.tsv <out> <seed>` for seeds `0` and `7`; `cmp` of the transcripts: identical (SHA-256 `24d971b2…46a5` and `0c7b1259…cbc2`, as Slice 2 recorded). The kernel image's loaded bytes (`objcopy -O binary`) are identical to the base's (`67d3cfc1…672c`); the ELF file differs only outside its loaded segments, as Slice 1 found. The guest builds to the committed `digest.wasm` byte for byte, before and after this slice |

**Whole Verify block, final branch:** both `fmt --check`s and all clippy runs exit 0; `fuzz` ends `no crash`; the workspace's tests with `test-trust` and `test-switches` pass 460 of 460 with `--no-fail-fast` (the first run failed `ledger::fixture_tests::local_4_2_generate`, the known timing flake, which differed only in `answer 6` against `answer 7` of a session-timing message, and passed on the rerun); `release_switches` passes 5 of 5 without the feature. Also run: root `cargo fmt --all --check`, `cargo xtask layout`, `lines` and `docs`, and `cargo deny --all-features check` from `hyalos-os/` (advisories, bans, licences and sources ok). Root-workspace tests were not run: this slice changes no file in it, and CI is the check.

**CI dry runs:** see the pull request's history: one pushed commit plants a clippy warning and one plants a handler panic, each is seen to fail CI's `hyalos-os` job, and both are removed before merge.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Each file on the `unsafe` list (`design.md` §2) carries a file-level allow | The eight listed files do, and so do three test files not on the list: `audit/tests/no_stand_in.rs`, `verify/tests/alone.rs` and `verify/tests/real.rs` | The verifier pack added them after the list was written. Each needs `unsafe` for `env::set_var` or `pre_exec`, and under `deny` they would not compile. Slice 10's list in `crates/xtask` should add them or move them out | n |
| Pattern: the handler fuzzer at `kvm.rs:329` | At `:581` | Line drift from earlier slices and formatting | n |
| Suggested path 2: fix lints in separate commits by crate | Done, but the commit for each crate also adds its `[lints]` opt-in and its file-level allows, and the workspace's lint table goes in with `attest`'s | A file's lint fix and its allow are in the same file, and the repository has no interactive staging here; each commit is still one crate | n |
| Clippy's findings are fixed | Two are allowed with a reason instead: `needless_range_loop` on the guest's one loop, and `too_many_arguments` on `declared_run` | The guest's iterator form compiles to other bytes, and the committed `digest.wasm`, whose hash every record names, is built from this source; gathering `declared_run`'s nine arguments in a type would change the code for no gain | n |
| Verify: clippy on the kernel image without features | Also with `selftest`, and the workspace with no feature as well as with `test-trust` | Each compiles other code (Slice 8's Discovery); the `selftest` build had a finding of its own | n |

## Decisions

- **The frame fuzzer's oracle.** It checks `read_frame` against the channel's definition written out in the test (`frame_by_definition`), and every decoder by canonical re-encoding: a frame that decodes must be exactly the encoding of what it decodes to. That catches an ignored byte, which a no-panic check alone would not. `split_boot` became `pub(crate)` so the `Boot` frame's parser is fuzzed too.
- **The frame fuzzer's seed** is `0x5eed_0505_c4a7_f00d`, with the handler fuzzer's xorshift generator. The holder's whole side of a run stays fuzzed by Slice 6's `core_s_04_4_a_fuzzed_channel_…` (1,500 streams).
- **Where the gates sit in CI.** Format and lint run right after the targets are added and before the long tests, so a lint fails in a minute or two. The handler fuzzer runs after `release_switches`, whose build without features it reuses, so it adds no compile. The frame fuzzer is a workspace test, so it needs no step. The step that linted only the four verifier crates is gone, since the new one covers them.
- **`_start`** became `pub unsafe extern "C" fn` with a `# Safety` section, the honest fix for clippy's `not_unsafe_ptr_arg_deref`; the image's loaded bytes did not change.
- **`clippy::all` at warn** lowers clippy's deny-by-default `correctness` group to warn, as the root's table does; CI's `-D warnings` still fails on them.
- No new repository ADR: the lint and fuzz gates are this pack's design (Modifier: infra), and ADR-0130's "That cleanup is later work" is this slice.

## Discoveries

- **For Slice 10:** the `unsafe` list should hold the eleven files that now carry `#![allow(unsafe_code)]` (each says so in a comment above it), or the three verifier tests should drop their `unsafe`. With the workspace at `deny`, any other file that gains `unsafe` already fails clippy and the build, so `cargo xtask hyalos-unsafe` adds the `SAFETY` comment rule and the list's record. The kernel image's `_start` now has a `SAFETY` comment; the other blocks were not reviewed for one here.
- **For Slice 10 and 12:** `digest.wasm` is reproducible from `guest/src/lib.rs` on this toolchain (`cargo build --release --locked --target wasm32-unknown-unknown` in `guest/`, then `cmp`). No check holds that today; a CI step or the review package's build instructions could.
- **CI time:** the new lint step ran cold in about 24 s of wall time for the three workspace clippy runs on a 10-core host (2 min of CPU), so on CI's runner expect one to three minutes cold and well under one warm; the kernel image's and guest's clippy add under a minute. The old step it replaces linted four crates. The handler fuzzer runs in 0.3 s and the frame fuzzer in about 1 s. The job should stay well inside its 20 minutes; the pull request's runs give the real figure.
- `ledger::fixture_tests::local_4_2_generate` flaked once here, as on the base: a timing-dependent message names `answer 6` or `answer 7`.
