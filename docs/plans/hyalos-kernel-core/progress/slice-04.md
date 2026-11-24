# Slice 4 progress

- **Status:** done
- **Issue:** #460
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked at base commit `bf6db82` (Slice 3 merged). Symbols were found by name, as `context.md` asks.

| Citation | Result |
|---|---|
| `design.md` §2, **Host probe** (the contract) | Holds: nothing in the code contradicted it. The real start state (`start_state`) and CPUID list (`allowed_cpuid`, with the `IA32_APIC_BASE` write) exist to reuse |
| ADR-002, amended by ADR-009 | Holds: `RDSEED`, `RDRAND` and `RDPID` are host-dependent, reported by the probe, and do not refuse a run |
| `hyalos-os/monitor/src/main.rs:192–197`, one machine per case (plan's **Pattern to follow**) | Differs by line only: the self-test probe loop is in `run_attacks`, near `:279`, reading `PROBES` (Slice 2) |
| `hyalos-os/monitor/src/main.rs:105`, replay | Holds: `replay` at `:105` |
| `hyalos-os/monitor/src/profile_tests.rs:51`, the tests build their own self-test kernel | Holds: `built_kernel` at `:51`, behind `selftest_kvm_backend` |
| `hyalos-os/monitor/src/kvm.rs`, `run` builds the machine | Holds: the setup is inline in `run` (`:303–331` at base), so this slice moved it into `machine` to share it with the probe |
| `hyalos-os/kernel-image/src/main.rs`, `probe` with bytes 1–18 and `CPUID_REPORT` | Holds, as Slice 3 left it |
| Slice 3's stopgap in `monitor/src/main.rs`: `HOST_DEPENDENT`, `host_cpu`, `traps_randomness`, `judge` | Holds, as Slice 3's Discoveries describe |
| `hyalos-os/RECORD.md` | Holds: it said nothing about hardware randomness before this slice |

No difference touches this slice's contracts.

## Acceptance

Tests are in `hyalos-os/monitor/src/probe_host_tests.rs`. They ran locally on KVM with `HYALOS_REQUIRE_KVM=1`, so none could skip. The development host (Intel Core Ultra 9 185H, `kvm_intel`, WSL2, nested KVM) gave `RDTSC blocked`, `RDTSCP blocked`, `RDRAND blocked`, `RDSEED reachable`, `RDPID blocked`, exit 1.

| ID | How it was shown |
|---|---|
| S-03.3 | `core_s_03_3_the_probe_reports_each_instruction_as_the_kernel_meets_it`: on this host's KVM the probe gives the five instructions in order, prints one `<name> blocked|reachable` line each, exits `1` exactly when one is reachable, and each verdict equals how the self-test kernel (a real kernel image) meets the same instruction: `blocked` where it ends `KernelFault`, `reachable` where it reaches the unknown request (`BadHypercall`). `core_s_03_3_the_command_prints_five_lines_and_exits_0_or_1` runs the built `task-kernel probe-host` with an empty environment: five lines, nothing on standard error, exit `0` or `1` as the lines say. `core_s_03_3_a_failed_control_is_exit_2_and_never_blocked`: a `nop` control that faults, a `ud2` control that reaches the port, and a machine that fails mid-way each give exit `2` and no verdict line. Verify's `cargo run … probe-host` printed the five lines above and exited `1` |
| S-03.3.e1 | `core_s_03_3_e1_a_host_where_every_instruction_faults_is_all_blocked_and_exits_0`: a faked host (the design's level for this row), all five `blocked`, exit `0` |
| S-03.3.e2 | `core_s_03_3_e2_a_host_where_rdseed_runs_reports_it_reachable_and_exits_1`: a faked host, `RDSEED reachable`, exit `1`. The real development host shows the same, through the S-03.3 tests and Verify's run |
| S-03.3.e3 | `core_s_03_3_e3_no_access_to_kvm_names_the_cause_and_exits_2`: a missing device gives `could not test: <path> does not exist, so this host has no KVM to test`, a device the process may not open gives `could not test: no permission to open <path>`, both exit `2` with nothing on standard output. Dry run: `unshare -rm sh -c 'mount -t tmpfs none /dev && task-kernel probe-host'` printed `could not test: /dev/kvm does not exist, so this host has no KVM to test` and exited `2` |
| S-03.4 | `core_s_03_4_randomness_cannot_change_a_verified_request`: on KVM, the self-test kernel's byte `0x81` reads `RDSEED` and asks a question holding its value, and replay refuses that record with `ReplayDiverged`. Byte `0x82` reads `RDSEED`, asks a fixed question and replays. Both end with the fixed result `done`, so the question is the only thing `RDSEED` can change |
| S-03.4.e1 | `core_s_03_4_e1_a_record_whose_question_rdseed_varied_is_refused_as_diverged`: two runs ask different questions with the same result, so `RDSEED` changed the request, and replay refuses the record as diverged |
| S-03.4.e2 | `core_s_03_4_e2_a_kernel_that_reads_rdseed_and_changes_nothing_replays`: `Ok(())` |
| 4.1 | `hyalos-os/RECORD.md` gains a bullet under **What a pass does not prove**, saying that `RDSEED` may stay reachable, and `RDRAND` and `RDPID` too where KVM cannot trap them (ADR-009), and a **The host probe** section with its output, exits and controls, which says hyalOS does not claim that a taken-over kernel lacks hardware randomness on every host. `core_4_1_the_record_format_states_that_rdseed_may_stay_reachable` checks those sentences and that the file says none of `no hardware randomness`, `without hardware randomness`, `cannot reach hardware randomness` or `has no randomness` |

On a host where the probe measures `RDSEED` blocked, the S-03.4 tests check instead that the `RDSEED` kernel ends `KernelFault` before it asks anything, since there randomness cannot reach a request at all (see Deviations).

**The tests discriminate.** Each was run against a deliberate break, then restored:

- The self-test kernel's byte `0x81` changed to ask the fixed question: `core_s_03_4` and its e1 fail (e1 at its premise, the questions no longer differ), so they do not pass on a kernel that `RDSEED` did not change.
- The probe's `RDRAND` sequence replaced by a `nop`: the probe then says `RDRAND reachable` while the self-test kernel faults, and `core_s_03_3_the_probe_reports_each_instruction_as_the_kernel_meets_it` fails. (The faked e1 and e2 tests also failed, only because their fake then reads the sequence as the control.)
- The CPUID filter changed to offer `RDRAND`: the three S-03.2 KVM tests fail, along with S-03.1's, though the probe now measures `RDRAND` reachable (see Decisions: each S-03.2 test also checks the machine's CPUID hides the instruction).

**Verify**, run locally on KVM:

- `HYALOS_REQUIRE_KVM=1 cargo test … core_s_03_3`: 6 passed. `… core_s_03_4`: 3 passed.
- `cargo run --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel -- probe-host`: the five lines above, exit `1`.
- The whole workspace with `HYALOS_REQUIRE_KVM=1` and `--no-fail-fast`, after CI's two `attest` builds: 413 tests (Slice 3's 403 and 10 new), none ignored or skipped. 412 passed; `ledger::fixture_tests::local_4_2_generate` failed under the parallel load, the known flake, and passed when rerun alone.
- `task-kernel attacks` with the self-test kernel: 62 `PASS` lines and one `REACHABLE randomness: hardware seed (RDSEED): Err(BadHypercall) (the host probe measured it reachable, ADR-009)`, exit `0`.
- CI's other `hyalos-os` steps: `fmt` and `clippy` on the four new crates and `cargo deny` pass. `cargo xtask layout`, `lines` and `docs` pass. The root workspace's code is not touched; only `ci.yml` gains a step.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files: modify `monitor/src/main.rs`, `kvm.rs`, `kernel-image/src/main.rs`, `RECORD.md` | Also creates `monitor/src/probe_host.rs` and `probe_host_tests.rs`, changes `cpuid_tests.rs` and `ring_tests.rs`, and adds a CI step | The probe has its own module, and its tests their own file, as earlier slices' do. The other two test files used Slice 3's vendor prediction, which this slice replaces. The CI step runs the probe on each runner, so the log records what that host exposes (the open question in the README); it fails only on exit `2` | n |
| ADR-009's Decision: the S-03.2 tests require `KernelFault` on Intel with `kvm_intel`, accept a run on AMD, and claim nothing elsewhere | They now judge each randomness probe by the host probe's measurement of its own instruction: a fault is required where the probe measured it blocked, a run is accepted where it measured it reachable, and a host that cannot be measured fails the test (the KVM gate has already let it through) | ADR-009's Consequences asked the probe's measurement to replace the vendor prediction, and Slice 3's Discoveries addressed this to Slice 4. It is correct on Intel, AMD and a nested host whose outer hypervisor offers no `RDRAND` exiting alike. `traps_randomness` is removed; the host's vendor, brand and module are still written to the test output | n |
| `design.md` §2: a `nop` control must reach the port | A second control, `ud2`, must also fault, or the probe exits `2` | Without it a machine that never shut down cleanly could never show `blocked`, and a probe whose fault detection broke would print every instruction `reachable`. It only adds ways to exit `2` | n |
| S-03.4: a kernel that uses `RDSEED` | Shown where the host leaves `RDSEED` reachable, as on the development host. Where the probe measures it blocked, the tests check the kernel faults before asking | The examples' Given cannot occur on such a host: the read ends the machine | n |
| `PROBES` held `RDRAND` and `RDPID` only | It also holds `randomness: hardware seed (RDSEED)`, byte 19, host-dependent | So `attacks` reports `RDSEED` beside the others, and the S-03.3 test can compare the probe with the kernel for all five instructions | n |
| Verify's expectation | Held: `RDSEED reachable` in 64-bit ring 3 on the development host, as `context.md` §2 measured in 32-bit mode | — | n |

## Decisions

- **The machine is shared, the loader is not.** `kvm::machine` builds the memory region, CPUID list, `APIC` base and start state for both `run` and `kvm::probe`, so the probe cannot drift from what a kernel image gets. The probe's own loader writes a fixed sequence at the kernel's base, read-only and executable, and never reads an image, so Slice 8 can refuse raw images without touching it.
- **Endings.** A machine that reaches port `0x1000` is `reachable`; a `Hlt` or `Shutdown` exit is `blocked`, as `run` maps them to `KernelFault`; any other exit, a KVM error or a run past 2 s is a cause and exit `2`.
- **Messages are fixed text.** A cause names the device path, a compile-time constant, and an `errno` class, never a system error string or anything read from the host.
- **Exit status outside `main_inner`.** `probe-host` is dispatched in `main` before `main_inner`, since `main_inner` has only two endings.
- **The S-03.2 tests still catch a filter regression.** Judging by measurement alone would accept a run if the CPUID filter offered `RDRAND`, since the probe's machine would then run it too. So `randomness` also reads the machine's CPUID through the self-test kernel and requires the instruction's bit clear.
- **The `attacks` command's `REACHABLE` line** says whether the probe measured the instruction reachable or could not test the host, and no longer prints the vendor or KVM module.

## Discoveries

- **For Slice 6 (the runner):** `kvm::probe` builds a machine in the calling process, as `run` does. When the machine moves into the confined runner, the probe should either run there too or stay a separate, unconfined diagnostic; it holds no task input or credential either way.
- **The deadline signal.** `kvm::probe` uses the same process-wide `Ticker` as `run`, so on a thread other than the main one its 2 s limit may never interrupt the virtual CPU (Slice 2's Discovery). Its sequences cannot loop, so this has not mattered.
- **For Slice 12:** `RECORD.md` now states the limit and the probe. The package's Limits should cite it, and say `RDRAND` and `RDPID` beside `RDSEED` (ADR-009).
- **CI's hosts.** The new step prints each runner's result, which answers the README's open questions as runs accumulate: whether `RDSEED` is reachable on CI's Intel runners, and what an AMD runner shows.
- The faked-host tests decode each sequence by its bytes, so a change to a sequence in `INSTRUCTIONS` needs no change there, but a change to the controls' bytes does.
