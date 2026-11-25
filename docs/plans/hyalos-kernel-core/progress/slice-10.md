# Slice 10 progress

- **Status:** done
- **Issue:** #466
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked on branch `issue/466`, forked from `origin/main` at `5d72202` (Slice 9 merged). Symbols were found by name.

| Citation | Result |
|---|---|
| `context.md` §3, line budgets: `crates/xtask/src/main.rs:17`, `:21`, `:349` | Holds: `BUDGET`, `PART_BUDGETS` and `fn lines` at those lines on the base |
| Pattern: `count_rust` beside `lines` | Holds: it stops at a line starting `#[cfg(test)]` and skips blank and `//` lines. Every hyalOS file that has a `#[cfg(test)]` has it at column 0 at the end of the file, so nothing after it is code the count should see |
| `context.md` §1: the root forbids `unsafe` (`Cargo.toml:14`) | Holds: `[workspace.lints.rust] unsafe_code = "forbid"` at `:13–14`. xtask keeps its own `forbid` |
| `design.md` §2, **Line and `unsafe` checks**: the two checks' inputs and failure rules | Holds. Nothing in the repository implemented either |
| `design.md` §2: the `unsafe` list, seven files at `63a75ec`, and Slice 7's `runner.rs` | Differs: all eight hold `unsafe`, and so do three verifier tests the list predates (`audit/tests/no_stand_in.rs`, `verify/tests/alone.rs`, `verify/tests/real.rs`), as Slice 9 found. See Deviations |
| Slice 9 Discoveries: the lint table at `unsafe_code = "deny"`, eleven files with `#![allow(unsafe_code)]` | Holds: `hyalos-os/Cargo.toml:16` and the kernel image's and guest's own tables; the eleven files, each with its comment |
| Slices 6–7 Discoveries: `runner.rs`'s `close_range` and the test-only `kill`, `mmap` and `clock_gettime` | Holds: each with a `SAFETY` comment |
| ADR-007: about 4,700 lines on 2026-11-23, and the verifier pack adds at most 3,000 | Differs: 9,021 lines at this base. ADR-007's figure is an estimate (cold read C-06), and the budget is the contract; the count is under it, so the second stop condition does not trigger |
| `docs/adr/`: "the next free number" | `0133` |
| Stop: the root trusted-core count | Unchanged: 28,594 / 30,000 before and after, with the same parts and per-crate counts |

No difference touches this slice's contracts.

## Acceptance

Run locally on KVM with Landlock ABI 3, `HYALOS_REQUIRE_KVM=1` and `HYALOS_REQUIRE_LANDLOCK=1`.

| ID | How it was shown |
|---|---|
| S-05.1 | `cargo xtask lines` prints `hyalOS: 9021 / 12000 lines (75.2%)` and the count per crate after the trusted core's report, and exits 0. `core_s_05_1_hyalos_os_is_within_its_budget` holds the real tree to the budget; `core_s_05_1_tests_and_build_output_are_not_counted` pins the exclusions (`tests/`, `*_tests.rs`, after `#[cfg(test)]`, build output). CI's `lint` job runs `lines` on every pull request |
| S-05.1.e1 | `core_s_05_1_e1_code_within_the_budget_passes`: exactly 12,000 counted lines pass. Counter-checked on the tree: a planted `monitor/src/planted.rs` of 2,979 lines took the count to `12000 / 12000` and `lines` exited 0 |
| S-05.1.e2 | `core_s_05_1_e2_code_one_line_over_fails_naming_the_budget`. Counter-checked: one more line in the planted file made `lines` exit 1 with `hyalOS's own code is 1 lines over its budget of 12000 (ADR-0133)`. The file was removed |
| S-05.2 | `cargo xtask hyalos-unsafe` prints `hyalOS: unsafe only in the 11 listed files, each use with a SAFETY comment` and exits 0; `core_s_05_2_hyalos_os_keeps_the_unsafe_rules` holds the real tree to it. Its first run on the base found 19 uses with no `SAFETY` comment (7 in the guest, 12 in the kernel image), each given one here. `core_s_05_2_unsafe_with_no_safety_comment_fails`, `core_s_05_2_a_safety_comment_may_go_on_over_several_lines` and `core_s_05_2_a_stale_list_entry_fails` pin the comment rule and the list. CI's `lint` job runs it on every pull request |
| S-05.2.e1 | `core_s_05_2_e1_unsafe_in_a_listed_file_with_a_safety_comment_passes`: a block, an `unsafe impl` and `#[unsafe(no_mangle)]` with comments pass, and the lint's name and a comment do not count as uses |
| S-05.2.e2 | `core_s_05_2_e2_unsafe_in_an_unlisted_file_fails`, for each form `design.md` names and the keyword at a line's end. Dry run (the slice's Verify): an `unsafe` block with a `SAFETY` comment appended to `monitor/src/ledger.rs` made the check exit 1 with `monitor/src/ledger.rs:175: unsafe in a file not on the list`. Also: deleting the `SAFETY` comment at `monitor/src/kvm.rs:52` made it exit 1 with `monitor/src/kvm.rs:52: unsafe with no SAFETY comment within 3 lines above`. Both were restored |
| 10.1 | `docs/adr/0133-hyalos-has-a-line-budget-and-a-list-of-unsafe-files.md`, Proposed, and its row in `docs/adr/README.md` |

Verify, run on the final code:

- `cargo xtask lines`: exit 0; the trusted core `28594 / 30000`, unchanged, and hyalOS `9021 / 12000`.
- `cargo xtask hyalos-unsafe`: exit 0.
- `cargo test --locked -p xtask core_s_05`: 10 passed.

What CI runs, also run, all exiting 0:

- Root: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --locked -p xtask` (30 unit tests, 2 determinism-lint tests, 12 release-gate tests), `cargo xtask layout`, `lines`, `hyalos-unsafe` and `docs`, and `cargo deny check`. The rest of the root workspace's tests were not run: this slice changes no root crate but `xtask`, which no other crate uses.
- hyalOS, every step of CI's `hyalos-os` job in order: the two `attest` builds; **Format and lint every crate** (all three `fmt --check` and all six clippy runs); the workspace's tests with `test-trust` and `test-switches`, 460 passed, none failed, ignored or skipped (`HYALOS_REQUIRE_KVM=1`, `HYALOS_REQUIRE_LANDLOCK=1`; the `local_4_2_generate` flake did not occur); `release_switches`, 5 passed; the handler fuzzer (`100000 random requests: 7075 answered, 92925 refused, no crash`); `probe-host` (exit 1: `RDSEED reachable`, the others blocked, a stated limit the step accepts); `cargo deny --all-features check` from `hyalos-os/` (`advisories ok, bans ok, licenses ok, sources ok`); and `check-verifier-rebuild.sh` (two builds identical).

The new comments change no code that runs. The guest built from the changed source is byte-identical to the committed `digest.wasm` (`cmp`). The kernel image's loaded bytes (`objcopy -O binary`) are identical to the base's, with and without `selftest`; the ELF file differs only in its symbol and string tables, which are not loaded.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `design.md` §2: the list "starts with" seven files, and Slice 7 may add `runner.rs` | The list holds eleven: those eight and the three verifier tests that need `env::set_var` (`audit/tests/no_stand_in.rs`) or `pre_exec` (`verify/tests/alone.rs`, `verify/tests/real.rs`) | The verifier pack added them after the list was written, and the standard library has no safe form of either call. Rewriting another pack's tests to run a child process instead would change what they test. The design gives the list's starting set and where it lives, not a closed set, and the check's inputs and rules are as written | n |
| `design.md` §2: a `SAFETY` comment "within the three lines above" | A `// SAFETY` comment on the use's own line also counts, and a comment that spans several lines counts from its last line | Three uses in a row under one comment (the guest's buffers) or an `unsafe fn` under its `unsafe impl`'s comment would otherwise need a copy of it per line. The rule is no weaker: a non-comment line still breaks the reach | n |
| `design.md` §2: the keyword followed by `{`, `fn`, `impl`, `trait`, `extern` or `(` | Also at the end of a line | What follows on the next line cannot be read by a line check, so the bare keyword fails closed. rustfmt never writes it | n |
| Suggested path 1: a hyalOS counter with `count_rust`'s rules | In a module of its own, `crates/xtask/src/hyalos.rs`, which `lines` calls after the trusted core's count; the `unsafe` check is there too | `main.rs` was already 708 lines; `refs.rs` is the pattern for a check of its own | n |
| Build output | `target/` and hidden directories under `hyalos-os/` are not read | Built crates and the kernel image's and guest's own `target/` hold generated Rust that is not hyalOS's code | n |

## Decisions

- **The three verifier tests join the list** (see Deviations). Their comments, and those of the eight other files, now point at `UNSAFE_FILES` in `crates/xtask/src/hyalos.rs` instead of `design.md` §2.
- **A listed file that no longer exists fails the check,** so the list cannot keep a stale entry.
- **Where CI runs it.** Both checks run in the `lint` job, which runs on every pull request, next to the trusted core's `lines`; the step is renamed `Trusted-core and hyalOS size (ADR-0017, ADR-0133)`. The `hyalos-os` job is unchanged.
- **The ADR is `0133`,** Proposed; the PR's last commit before merge sets it Accepted, as the repository's rule says. `crates/xtask`'s budget table (`PART_BUDGETS`) and dependency edges are unchanged, so ADR-0017 needs no new record.
- **The kernel image's `#[unsafe(no_mangle)]` and its fn-pointer type** (`unsafe extern "C" fn` in the self-test probe) match the design's forms, so each has a `SAFETY` comment, as the attribute's own `unsafe(...)` asks.

## Discoveries

- **For Slice 12:** the review package can point the reviewer at `UNSAFE_FILES` for every `unsafe` in hyalOS: eleven files, every use with a reason. The test-only uses (`runner.rs`'s hostile acts under `test-switches`, the kernel image's `selftest` probes) are checked the same as the rest.
- **For Slice 12 or a capsule:** hyalOS is at 9,021 of 12,000 lines: `attest` 3,222, `monitor` 2,773, `audit` 1,817, `kernel-image` 428, `attest-check` 370, `verify` 192, `guest` 171, `profile` 48. About 3,000 lines of room are left.
- **`CLAUDE.md`'s map** lists `crates/xtask` under ADRs 0017 and 0111; it could add 0133. Not changed here.
- The check is textual and does not parse Rust, so `unsafe {` inside a string literal or block comment would fail it. None exists today; the ADR states the limit.
