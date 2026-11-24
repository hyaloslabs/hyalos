# Slice 6 progress

- **Status:** done
- **Issue:** #462
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked on branch `issue/462`, forked from `origin/main` at `4ee6273` (Slice 5 merged). Symbols were found by name, as `context.md` asks.

| Citation | Result |
|---|---|
| `design.md` §2, **Runner channel `hyalos.runner/1`** and its failure mapping (the contract) | Holds: no runner existed, and nothing in the code contradicted the frame layout, limits, invariants, deadline or failure mapping |
| `design.md` §4, **Machine runner** (the contract) | Holds |
| `design.md` §1, **Starting the runner** and **Order inside the runner** | Holds. Landlock and seccomp are Slice 7's, so this slice's order is open KVM, read `Boot`, build, run |
| `hyalos-os/monitor/src/kvm.rs:181`, `answer` and its five requests (**Pattern to follow**) | Differs by line only: `answer` was at `:237` at base, moved by Slices 2 to 4 |
| `context.md` §2: `kvm.rs:181–236` "Kept. Moves behind the confinement channel" | Holds at `:237–292` |
| `Host` in `hyalos-os/audit/src/replay.rs:94`, used by the monitor | Holds |
| `kvm.rs:282`, the timer path that gives `Reason::Timeout` (the **Deadline** paragraph) | Differs by line only: `run`'s deadline check was at `:380–381` |
| `hyalos-os/monitor/src/main.rs:267`, the chain's `refused:` strings | Differs by line only: `declared_run`'s `format!("refused:{reason:?}")` was at `:337` |
| `context.md` §2: `main.rs:351–395`, `live` runs the task and writes the chain in one process | Differs by line only: the `live` arm was at `:413` |
| `context.md` §2: `live.rs:176` and `:202` start the attestation tool and `gh` | Holds: both stay in the holder |
| `live_tests.rs:430`, tests find the `task-kernel` binary beside the harness | Holds (`:430`) |
| Slice 2 and 4 Discoveries: the deadline's `SIGALRM` reaches only the main thread | Holds at base: `Ticker` in `kvm.rs`, used by `run` and `probe`. `core_s_02_every_hostile_kernel_ends_as_before_in_ring_3` filtered out `spin forever` |
| Slice 3 and 4 Discoveries: `kvm::machine` holds the CPUID list and the `IA32_APIC_BASE` write with its `NoKvm` refusal, shared by `run` and `probe` | Holds; `machine` is unchanged and now built only by the runner and the host probe |
| Slice 5 Discoveries: `attest/probes/two_machines.sh` exports `HYALOS_KERNEL` for its `live` call, and `attest/tests/canary.rs` runs it with stubs | Holds. The script's invocation of `live` is unchanged, so neither needed a change |

No difference touches this slice's contracts.

**Base, recorded before any edit** (kept outside the repository): at `4ee6273`, `task-kernel attacks` with the kernel and the self-test kernel printed 63 lines, 62 `PASS` and `REACHABLE randomness: hardware seed (RDSEED): Err(BadHypercall) (the host probe measured it reachable, ADR-009)`, exit `0`. `task-kernel run` of `guest/digest.wasm` on `evidence/fixtures/issues.tsv` gave SHA-256 `24d971b2…46a5` (seed 0) and `0c7b1259…cbc2` (seed 7) on KVM.

## Acceptance

Tests are in `hyalos-os/monitor/src/runner_tests.rs`. The channel tests drive the holder's side (`runner::serve`) from a hand-built runner on a socket pair, so they need no KVM. The runner tests start the real `machine-runner` from the `task-kernel` binary beside the harness, behind Slice 1's KVM gate. They ran locally with `HYALOS_REQUIRE_KVM=1`, so none could skip.

| ID | How it was shown |
|---|---|
| S-04.1 | `core_s_04_1_every_kvm_run_executes_in_a_machine_runner`: the KVM backend calls `runner::run`, `kvm::machine_run` has one caller (the `machine-runner` command), and the only other machine in the monitor is the host probe's. `core_s_04_1_a_kernel_that_spins_is_stopped_at_the_deadline`: the raw `spin forever` kernel ends with `Timeout` after its 1 s limit under the test harness, which Slice 2 could not test. `core_s_04_1_a_live_run_checks_the_runner_before_its_chain`: `machine-runner --check` passes on this KVM host, a `live` run on a real repository with a kernel configured passes it and stops at the next step (no home) with no ledger, and the command refuses any other argument |
| S-04.1.e1 | `core_s_04_1_e1_the_vcpu_runs_in_a_child_with_only_its_kvm_handles_and_the_channel_open`: while the digest guest waits on its first question, `/proc/<runner>` shows a child of the test process with one thread, an empty environment, `/` as its directory, and open files exactly `/dev/kvm`, `anon_inode:kvm-vm`, `anon_inode:kvm-vcpu:0`, the channel's socket twice (standard input and the copy the runner reads and writes) and `/dev/null` twice. Counter-check: without `env_clear` it fails |
| S-04.1.e2 | `core_s_04_1_e2_a_runner_that_exits_mid_run_ends_with_a_refusal_and_no_effect`: the runner is killed during the third question. The holder ends the run with `KernelFault` at once, the source is asked nothing more, and the guest's effect never happens; the same run unkilled asks 40 questions and sends its effect. `core_s_04_1_e2_a_channel_that_closes_mid_run_performs_no_effect`: a runner that closes the channel after an effect's type check ends with `KernelFault` and no effect. Dry run: see below |
| S-04.4 | `core_s_04_4_the_holder_reprints_only_a_well_formed_trace_line` (under `HYALOS_TRACE` only the runner's exact timing line, parsed, is printed again; an escape, a second line or any other byte prints nothing), `core_s_04_4_a_malformed_message_ends_the_run_with_no_effect` (another version, unknown types, `Boot` and replies from the runner, fields of the wrong length, a reason outside the table or one only the holder gives, a truncated frame, a body, question or result over `Host`'s limits), `core_s_04_4_an_out_of_order_message_ends_the_run_with_no_effect` (nine cases), `core_s_04_4_the_holder_keeps_hosts_rules`, `core_s_04_4_the_holder_ends_a_silent_or_slow_runner_at_the_deadline`, `core_s_04_4_the_reason_table_is_closed_and_every_message_round_trips`, and the frame fuzzer `core_s_04_4_a_fuzzed_channel_never_panics_or_performs_an_unasked_effect` (1,500 seeded streams; no panic, at most one effect and only after its type check, every refusal a reason a run already gives, and the streams reach effects, questions and finished runs). Each malformed or out-of-order case follows a frame that opened the way to an effect, and asserts none happened. Counter-checks: without the holder's length re-check the malformed test fails; accepting any reason in `Failed` fails it and the fuzzer; mapping the deadline to `KernelFault` fails the deadline, spin and 6.1 tests |
| S-04.4.e1 | `core_s_04_4_e1_a_message_over_the_size_limit_is_refused`: a frame one byte over the runner's limit (64 KiB + 64) is refused before its body is read and no effect follows, as are lengths 0, 1 and `u32::MAX`; the largest frame a run needs, a 65,536-byte effect, is performed. `core_s_04_4_e1_the_runner_refuses_a_boot_or_reply_over_its_limit`: the runner's reader refuses a `Boot` over 8 MiB and a reply over 16 MiB + 64 and reads one at the limit, and the holder refuses to build a `Boot` over its limit |
| S-04.4.e2 | `core_s_04_4_e2_a_second_effect_request_is_refused`: a second `PreEffect` is refused with `EffectLimit` by `Host`'s own rule, and a second `Effect` after one type check with `KernelFault`; one effect is performed either way |
| 6.1 | `core_6_1_the_attacks_command_prints_the_previous_commits_lines`: the built `task-kernel attacks`, with the kernel and the self-test kernel, prints exactly the 63 lines `4ee6273` printed, pinned in the test, with the three host-dependent randomness lines in the form that commit prints for this host's measurement. Dry run: the same command from the base binary and from this branch's, `diff` empty |

**Dry runs** (the slice's Verify block):

- The Slice 2 differential on this branch: `task-kernel run` on KVM printed `ok: 40 inputs, 40 inferences, 1 effect, fuel 29845` (seed 0) and `… fuel 30886` (seed 7), SHA-256 `24d971b2…46a5` and `0c7b1259…cbc2`, byte-identical to the base. `core_s_02_1_e1` also pins them.
- Killing the runner mid-run (S-04.1.e2): `HYALOS_KERNEL` set to the raw `spin forever` kernel, `task-kernel run` with its 30 s limit, and its one child (`task-kernel machine-runner`) sent `SIGKILL` after 1 s. The command printed `refused: KernelFault` and nothing on standard output, exited `1` 1.1 s after it started, and wrote no transcript.

**Verify**, run locally on KVM after CI's two `attest` builds:

- `HYALOS_REQUIRE_KVM=1 cargo test … core_s_04_1`: 6 passed. `… core_s_04_4`: 9 passed. No `skipped:` line.
- The whole workspace with `HYALOS_REQUIRE_KVM=1` and `--no-fail-fast`: 435 tests (Slice 5's 419 and 16 new), none ignored and no `skipped:` line. 434 passed; `ledger::fixture_tests::local_4_2_generate` failed under the parallel load, the known flake, and passed when rerun alone.
- CI's other `hyalos-os` steps: `fmt --check` on `profile`, `audit`, `attest-check` and `verify`, their `clippy -D warnings`, and `cargo deny --all-features check` from `hyalos-os/` (advisories, bans, licences and sources ok). `clippy` on the monitor reports the same warnings as at base and none in the new files. Root checks: `cargo xtask layout`, `lines` and `docs` pass. The root workspace is not touched.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files: create `runner.rs`, `runner_tests.rs`; modify `kvm.rs`, `main.rs` | Also `ring_tests.rs` and `ABI.md` | Its hostile-kernel test now includes `spin forever` again, which Slice 2 had to leave out (Slice 2's Discoveries). `ABI.md`'s `KernelFault` row, outside the pinned sections, now names a runner that dies, closes its channel or breaks `hyalos.runner/1` | n |
| `design.md` §2, `Reply` carries a record | A record reply is its length and its bytes, and leaves the bytes out only for a record longer than the heap (16 MiB) | The reply's limit is the heap's size. `answer` needs a record's length to choose `-2` or `BadPointer`, so a record too long to send still gives the reason it gave before, and could never be written anyway | n |
| `design.md` §2, `Failed` carries "closed reason value" | The holder accepts `Failed` only with a reason the machine and its requests give (`RUNNER_REASONS`), and between a type check and its bytes only `TooLarge` or `BadPointer`. Any other is `KernelFault` | The rest are the holder's to decide, so a runner claiming one is breaking the protocol | n |
| Failure mapping: `machine-runner --check` "applies both confinements and exits" | It starts a runner with the runner's environment and opens KVM, and a `live` run on KVM asks it after `record_is_fresh`, before the ledger opens | Slice 7 adds the confinements. Without KVM a `live` run is now refused before its intent link, where before it wrote an intent and a `refused:NoKvm` result: no new `refused:` value, one fewer link | n |
| NFR: cold start as `HYALOS_TRACE` reports it | `HYALOS_TRACE` now prints two lines: the runner's, with the old fields `memory+load`, `vm setup` and `boot to first call`, and the holder's, `runner started`, `start to first call`, `calls`, `total`. For the runner's line the holder passes `--trace` and gives that runner a socket of its own as standard error. After a run that ended well it reads at most 256 bytes back without waiting and prints the runner's line again from its three parsed numbers, only when the bytes are exactly that line, so no byte the runner writes reaches the holder's output (`core_s_04_4_the_holder_reprints_only_a_well_formed_trace_line`) | The runner's environment is empty, so it cannot read `HYALOS_TRACE`, and its standard error is otherwise null. Review found that inheriting the holder's standard error let the runner write anything there. Test-only, like `HYALOS_TRACE` itself, which Slice 8 gates | n |
| — | `HYALOS_HUGE` no longer reaches a run's memory | It is read where the memory is mapped, in the runner, whose environment is empty. Slice 8 removes or gates it | n |
| Verify passes on its own | It needs CI's two `attest` builds first | As Slices 1 to 5 found | n |

## Decisions

- **The deadline.** The holder sets each read's timeout to the time the run has left and ends the run with `Timeout` when it passes, killing the runner; a frame that arrives one byte at a time cannot stretch it. The runner has no timer, and the 10 ms `SIGALRM` ticker and the teardown thread left `run`. The host probe keeps the ticker: its sequences cannot loop and it holds no task input (Slice 4's Discoveries left it as an unconfined diagnostic).
- **A refusal the holder decides** (`UnknownSchema`, `UnknownEffect`, `EffectLimit`, a model or effect failure) is sent to the runner as a `Refused` reply and ends the run with that reason at once, without waiting for the runner.
- **Order of reasons.** The holder checks the guest's size before starting a runner, and the runner opens KVM first but reports its failure only after the image is checked, so a run with two faults gives the reason it gave before.
- **Teardown.** The runner reports how the run ended before it tears its machine down, which takes this host 17–37 ms. The holder kills it and reaps it on another thread, and the program waits for those threads before it exits, so a command normally leaves no runner behind and the next command starts on a quiet host, as before. The wait is bounded (5 s, `REAP_BOUND`, `runner_waiting_for_reapers_is_bounded`): a teardown that is stuck cannot hang the program after its outcome is decided, and a runner that outlives the bound is reaped by init once the program exits.
- **The runner's channel** is its standard input and one copy of it: the standard library gives no safe way to write to descriptor 0, and the copy adds no `unsafe`. Slice 7's seccomp filter should allow both descriptors.
- **Formatting.** `runner.rs` and `runner_tests.rs` are `rustfmt`-formatted and clippy-clean; the rest of the monitor is left for Slice 9.
- **No repository ADR.** No ADR under `docs/adr/` owns the monitor's process model; Slice 7 adds the one for the confined runner (7.1).

## Discoveries

- **Cold start: the stop condition's metric holds, the whole start does not.** Measured interleaved with the base binary, 21 runs per round, two rounds, seed 0, the committed digest guest: `boot to first call` (the NFR's metric, first `KVM_RUN` to the first request) had medians of 2.12 and 2.14 ms at base and 2.03 and 2.01 ms here, within the 20% (Slice 2 measured 2.25 ms). But the holder's whole start, from the run's start to its first request, was 6.97 and 7.11 ms here against the base's `memory+load` + `vm setup` + `boot to first call` of 3.77 and 3.90 ms (+85%), and a whole run went from 4.5 ms to 13.5 ms: starting the runner process (about 0.9 ms to spawn and 0.4 ms to reach `main`), moving the 2 MB kernel through the socket (about 1.5 ms), and each of the run's 124 requests crossing two processes (about 45 µs each). Overlapping the machine's setup with reading `Boot`, and busy-waiting on the socket, were tried and gained under 5% here, so neither is kept. The reviewer and Slice 7 should know the whole start is slower.
- **For Slice 7: the runner's system calls.** Read from the code, not traced (this host has no `strace`): before its machine starts the runner copies standard input (`fcntl`), maps its 32 MiB memory and makes `machine`'s KVM ioctls; after, it should need only `KVM_RUN`, `read` and `write` on the channel, memory calls for its frame buffers, and `exit_group`. It sets no timeout and no socket option. Slice 7 should measure the real list.
- **For Slice 7: `--check`** is where Landlock and seccomp availability should be added, and the order inside the runner is already open KVM, then read `Boot`.
- **The deadline is now reliable everywhere** a KVM run happens, including `live`'s per-call reader threads: the runner is a separate process and the holder's timeout does not depend on which thread a signal reaches.
- **Hosts.** Nothing in the runner depends on the CPU vendor. The randomness probes are still judged by `probe-host`'s measurement, and the 6.1 test prints the base's line for that measurement on Intel and AMD alike.
