# Slice 8 progress

- **Status:** done
- **Issue:** #464
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked on branch `issue/464`, forked from `origin/main` at `d41f187` (Slice 7 merged). Symbols were found by name, as `context.md` asks.

| Citation | Result |
|---|---|
| Slice 8 **Contracts**: the `attacks` command, `HYALOS_SELFTEST_KERNEL`, `HYALOS_TRACE`, `HYALOS_HUGE`, raw images, the host probe's sequences | Holds: each existed as the table's "With `test-switches`" column describes, and none was gated yet |
| `context.md` §2: `kvm.rs:112–119`, the raw, non-ELF image mapped writable and executable | Differs by line only: `load` at `:122`, the raw branch at `:124–129` |
| `context.md` §2: `kvm.rs:41`, `HYALOS_HUGE` | Differs by line only: `Ram::new`, `:51` |
| `context.md` §2: `kvm.rs:240`, `HYALOS_TRACE` | Differs: since Slice 6 the holder reads it (`runner::run`, `runner.rs:589`) and starts the runner with `--trace`; the runner's `kvm::boot_and_run` prints the line. Not a contract difference: the contract says only that a default build does not read it |
| `context.md` §2: `main.rs:198`, `HYALOS_SELFTEST_KERNEL` for `attacks` | Differs by line only: `run_attacks`, `:266` |
| `context.md` §3: `main.rs:150` (`hostile_kernels`), `:181`, `:192` | Differs by line only: `:154` |
| Pattern: the kernel feature gate, `kernel-image/Cargo.toml:14` | Holds (`selftest = []`) |
| `design.md` Host probe: fixed sequences placed by the probe's own loader | Holds: `kvm::probe` writes its sequence with `Ram::write` and never calls `load` |
| `design.md` **Modifier: infra**, the `test-switches` feature and CI's test step at `ci.yml:248` | Holds: the feature exists (Slice 7), the step is at `:250` and already enables it |
| Slice 7 Discoveries: nothing runs the monitor without the feature in CI | Holds; this slice adds the step |
| ADR-0132: `write` only to standard error, only under `--trace` | Holds |

No difference touches this slice's contracts.

## Acceptance

Tests are in `hyalos-os/monitor/tests/release_switches.rs`, an integration test that runs the `task-kernel` binary cargo builds beside it. Built without the feature (as CI's new step and the Verify block's first command run it) each test checks the "Default build" column; built with `task-kernel/test-switches` (the workspace run) the same test checks the other column, so the feature is shown to be the only difference. The KVM tests ask the binary's own `machine-runner --check` whether a confined machine can run, and fail rather than skip when `HYALOS_REQUIRE_KVM` or `HYALOS_REQUIRE_LANDLOCK` is set. They ran locally with both set (KVM, Landlock ABI 3), so none could skip.

| ID | How it was shown |
|---|---|
| S-05.4 | `core_s_05_4_e1_…`, `core_s_05_4_e2_…` (below), and: `core_s_05_4_trace_and_huge_change_nothing`: a run of the committed guest on the real kernel image with `HYALOS_TRACE=1` and `HYALOS_HUGE=1` prints the same standard output and writes the same transcript as one without, and prints nothing to standard error (with the feature, it prints the `trace ms:` line). `core_s_05_4_every_switch_is_named_only_in_code_built_with_the_feature` (structural): the three variables are read only in `monitor/src/switches.rs`, which `main.rs` declares under `#[cfg(feature = "test-switches")]`, and the tests name them only inside tests under the same `cfg`. `core_s_05_4_a_release_runner_refuses_trace` (in `runner_tests.rs`): `machine-runner` parses `--trace` only with the feature, so a release runner's filter never allows `write`. `core_s_05_4_ci_runs_this_file_without_the_feature` (structural): CI's `hyalos-os` job runs this file without the feature, after the workspace's tests with it, with KVM and Landlock required |
| S-05.4.e1 | `core_s_05_4_e1_attacks_reads_no_kernel_file`: with `HYALOS_SELFTEST_KERNEL` set, and separately with `HYALOS_KERNEL` set, each to a named pipe a thread of the test holds open for writing, a release build's `attacks` exits `1` with exactly `refused: the attacks command needs a build with the test-switches feature`, prints nothing, and the pipe was never opened (opening one to read would block until the writer, so the program could not have ended). With the feature, the same command opens each pipe |
| S-05.4.e2 | `core_s_05_4_e2_a_raw_kernel_image_is_refused`: the real kernel image runs (control); a raw five-byte image (`mov dx, 0x80; out dx, eax`) makes `task-kernel run` print `refused: KernelFault`, exit `1` and write no transcript. With the feature it loads and ends `refused: BadHypercall`, so the refusal is the loader's, not the machine's |

**Counter-checks** (each broken in a scratch edit, run without the feature, then restored): letting `load` take a raw image in a release build fails `core_s_05_4_e2`; a release `attacks` that reads `HYALOS_KERNEL` before refusing fails `core_s_05_4_e1`; a release runner that prints its timings fails `core_s_05_4_trace_and_huge_change_nothing` (and `e2`, since the filter then kills it); reading `HYALOS_HUGE` in `kvm.rs` outside `switches.rs` fails the structural test; giving CI's new step `--features task-kernel/test-switches` fails the CI test.

**Dry run** (S-05.4.e1, the Verify block's last line): the default release binary (`cargo build --release -p task-kernel`), each switch set by hand: `attacks` with `HYALOS_SELFTEST_KERNEL=/nonexistent` printed `refused: the attacks command needs a build with the test-switches feature` and exited 1; `run` with `HYALOS_TRACE=1 HYALOS_HUGE=1` printed the same `ok: 40 inputs, 40 inferences, 1 effect, fuel 29845` as without, and nothing on standard error; `run` with a raw image printed `refused: KernelFault`; `machine-runner --trace` exited 2 (`BROKEN`, an unknown argument). `probe-host`, as CI's step runs it without the feature, still measured all five instructions (`RDSEED reachable`, the rest blocked, exit 1, as before on this host).

**Verify**, run locally on KVM after CI's two `attest` builds:

- `cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --test release_switches core_s_05_4`, with `HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1`: 5 passed, no `skipped:` line.
- `HYALOS_REQUIRE_KVM=1 cargo test --release --locked … --workspace --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches` (with `HYALOS_REQUIRE_LANDLOCK=1` too, `--no-fail-fast`): 459 passed, none failed, ignored or skipped, `release_switches` (5) and the monitor's 242 unit tests included. The first run, without `--no-fail-fast`, hit the known flake `ledger::fixture_tests::local_4_2_generate` (`fixture_tests.rs:242`) and stopped there; it passed on the rerun. Without the feature, `cargo test -p task-kernel --features test-trust`: 227, 10 and 5 passed, none skipped.
- `git grep -n 'HYALOS_SELFTEST_KERNEL\|HYALOS_TRACE\|HYALOS_HUGE' -- hyalos-os/monitor/src`: six lines in `switches.rs`, the three reads and their doc comments (compiled only with the feature), and `runner_tests.rs:1109`, inside `core_6_1_the_attacks_command_prints_the_previous_commits_lines`, which is under `#[cfg(feature = "test-switches")]`.
- CI's other `hyalos-os` steps: `rustfmt --check` on `runner.rs`, `runner_tests.rs`, `switches.rs` and `release_switches.rs`; `clippy` on the monitor with and without the feature reports the same warnings as at base and none in the new code; `fmt` and `clippy` on the four new crates unchanged. `cargo deny --all-features check` from `hyalos-os/`: no dependency change. Root checks: `cargo xtask layout`, `lines` and `docs` pass. The root workspace's code is not touched (only `ci.yml` and this pack), so its tests are CI's to run.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files: create `hyalos-os/monitor/tests/release_switches.rs`; modify `kvm.rs`, `main.rs`, `Cargo.toml`, `ci.yml` | Also `monitor/src/switches.rs` (new), `runner.rs`, `runner_tests.rs`, `ring_tests.rs` | Since Slice 6 the holder reads `HYALOS_TRACE` in `runner.rs` and the runner takes `--trace`, so both are gated there. One file that alone names the variables makes "each read is compiled only with the feature" a structural test rather than a review. Four tests that use raw images or the switches are now built only with the feature | n |
| `HYALOS_TRACE`: "Not read" in a default build | Not read, and the runner also refuses `--trace` | A release runner that cannot be asked to trace never has `write` in its filter. ADR-0132 allows `write` only under `--trace`, so this is tighter and consistent with it | n |
| Verify: "tests pass with none skipped" | The first workspace run hit the known flake `ledger::fixture_tests::local_4_2_generate`; the rerun passed | It fails on base too (Slice 7 found the same); see Verify | n |
| Verify passes on its own | It needs CI's two `attest` builds first | As Slices 1 to 7 found | n |

## Decisions

- **The release `attacks` command** is refused before it reads any argument or kernel file, with `refused: the attacks command needs a build with the test-switches feature` and exit 1, through `main`'s usual refusal path. The whole command is test-only, its guest attacks included: they are the same list `ring_tests` runs, and keeping half a command in release would leave two outputs to pin.
- **A raw image in a release build** makes `load` return `None`, which `boot_and_run` already maps to `KernelFault`, so no new reason or record shape appears.
- **The guest attack list, the probe list and their judges** (`attacks`, `PROBES`, `HOST_DEPENDENT`, `judge`, `blocked_here`, `Verdict`) are compiled in tests and with the feature, since the self-test kernel probes (`ring_tests`, `cpuid_tests`) use an ELF kernel and still run without the feature. `hostile_kernels` and `run_attacks` are built only with the feature.
- **No repository ADR.** No event, wire message, tool or schema changes, and ADR-0132's allow-list is unchanged; the feature's scope is the pack's `design.md` (**Modifier: infra**).
- **Formatting.** `switches.rs`, `release_switches.rs`, `runner.rs` and `runner_tests.rs` are `rustfmt`-formatted; `kvm.rs` and `main.rs` keep their style for Slice 9.

## Discoveries

- **For Slice 9 (lint):** without the feature, `cargo test -p task-kernel` now builds and its tests pass; four tests (`core_s_02_every_hostile_kernel_ends_as_before_in_ring_3`, `core_s_04_1_a_kernel_that_spins_is_stopped_at_the_deadline`, `core_s_04_4_the_holder_reprints_only_a_well_formed_trace_line`, `core_6_1_…`) exist only with it. Lints should run with and without `test-switches`, since each build compiles different code. The two warnings at base (`kvm.rs`'s `tick as usize`, `live_tests.rs:441`'s unused `Result`) are unchanged.
- **For Slice 12 (the package):** the package's build commands should say that the reviewed binary is built without `test-switches`, and that `attacks` exists only in a test build; `release_switches.rs` is the check.
- **The cold-start NFR** is measured with `--features task-kernel/test-switches` from now on, as the PRD says, since a release build prints no timings.
- `HYALOS_HUGE` has had no effect on a KVM run since Slice 6 (the runner's environment is empty); with the feature it still advises the memory `fuzz` maps in the monitor's own process.
