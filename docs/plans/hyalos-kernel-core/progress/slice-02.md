# Slice 2 progress

- **Status:** done
- **Issue:** #458
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked at base commit `9c6706b` (Slice 1 merged). Symbols were found by name, as `context.md` asks.

| Citation | Result |
|---|---|
| `hyalos-os/monitor/src/kvm.rs:263–264`, the whole supported CPUID list | Holds. Not touched here (Slice 3) |
| `hyalos-os/monitor/src/kvm.rs:268–273`, ring 0 start with only `CR4.PAE` | Holds |
| `hyalos-os/monitor/src/kvm.rs:144–174`, page tables with no user bit, in the unmapped first megabyte | Holds |
| `hyalos-os/monitor/src/kvm.rs:112–119`, the raw, writable and executable image for hostile kernels | Holds (`:114`). Kept: Slice 8 owns it |
| `hyalos-os/monitor/src/kvm.rs:289`, `Hlt` and `Shutdown` are `KernelFault` | Holds |
| `hyalos-os/monitor/src/kvm.rs:41`, `:240`, `HYALOS_HUGE` and `HYALOS_TRACE` | Holds |
| `hyalos-os/kernel-image/src/main.rs:95–109`, leaving through `out` on `0x1000` and halting after a fault | Differs by line only: `hypercall` at `:100`, `halt` at `:109`, moved by Slice 1's `ABI` constant |
| `hyalos-os/kernel-image/src/main.rs:260–278`, the self-test probes | Differs by line only: `probe` at `:266`, its call at `:258` |
| `hyalos-os/monitor/src/main.rs:150`, `:181`, `:192`, `:198`: `hostile_kernels`, `run_attacks` and `HYALOS_SELFTEST_KERNEL` | Holds |
| `hyalos-os/monitor/src/profile_tests.rs:65`, the KVM gate | Holds |
| `design.md` §2, **Machine start state**, and ADR-001 | Holds: nothing in the code contradicted the table |
| `context.md` §2, the 64-bit ring 3 throwaway run (seeds `0` and `7` identical, 51 `attacks` cases, 2.17 ms median) | Repeated: see below |

No difference touches this slice's contracts.

**Base, recorded before any edit** (kept outside the repository, as the shared contract asks): `task-kernel run` of `guest/digest.wasm` on `evidence/fixtures/issues.tsv` on the ring 0 kernel gave SHA-256 `24d971b2…46a5` for seed 0 and `0c7b1259…cbc2` for seed 7, on both backends, the same as Slice 1 recorded. `attacks` with the self-test kernel passed 51 of 51 lines. Boot to first call over 21 runs: median 2.25 ms.

## Acceptance

Tests are in `hyalos-os/monitor/src/ring_tests.rs`, plus two unit tests at the end of `hyalos-os/monitor/src/kvm.rs`. They use Slice 1's KVM gate, now able to build the self-test kernel too (`selftest_kvm_backend`), and ran locally on KVM with `HYALOS_REQUIRE_KVM=1`, so none could skip. The `core_s_02` filter runs 16 tests, all pass.

| ID | How it was shown |
|---|---|
| S-02.1 | `core_s_02_1_every_hostile_guest_ends_as_before_in_ring_3`: every guest on the `attacks` list ends with the same reason on the ring 3 kernel as on the process backend, as it did on ring 0. `core_s_02_every_hostile_kernel_ends_as_before_in_ring_3`: the raw hostile kernels end with their listed reasons (except the deadline case; see Discoveries). Differential dry run: seeds `0` and `7` from this branch, `cmp` against the base transcripts, identical |
| S-02.1.e1 | `core_s_02_1_e1_the_digest_guest_gives_the_base_commits_transcript_in_ring_3`: the bytes `task-kernel run` writes have the base commit's SHA-256 for seeds `0` and `7`, pinned in the test, and equal the process backend's. Verify's `run` printed `ok: 40 inputs, 40 inferences, 1 effect, fuel 30886` |
| S-02.1.e2 | `core_s_02_1_e2_a_guest_out_of_fuel_ends_as_on_the_base_commit`: `OutOfFuel` on both backends, as the base's `attacks` line `loop forever` showed |
| S-02.2 | `core_s_02_2_rdtsc_and_rdtscp_fault` |
| S-02.2.e1 | `core_s_02_2_e1_reading_the_time_counter_faults` |
| S-02.2.e2 | `core_s_02_2_e2_clearing_cr4_tsd_faults_at_the_write`. The probe sends an unknown request between the write and `RDTSC`, so a write that went through would end with `BadHypercall`. On the base's ring 0 machine it does end with `BadHypercall`, so the write was allowed there |
| S-02.3 | `core_s_02_3_control_registers_descriptor_tables_msrs_and_hlt_fault`: `CR4`, `CR3`, `CR0`, `LGDT`, `LIDT`, `WRMSR` and `HLT` |
| S-02.3.e1 | `core_s_02_3_e1_loading_other_page_tables_faults` |
| S-02.3.e2 | `core_s_02_3_e2_writing_a_model_specific_register_faults` |
| S-02.4 | `core_s_02_4_every_memory_probe_still_faults`: the seven memory probes fault and the control survives. `core_s_02_every_probe_ends_as_listed` runs the whole list. Dry run: `attacks` with the self-test kernel, 60 of 60 lines `PASS` (the base's 51 and the 9 new probes) |
| S-02.4.e1 | `core_s_02_4_e1_changing_its_own_code_faults` |
| S-02.4.e2 | `core_s_02_4_e2_the_control_probe_reaches_the_monitor`: `BadHypercall` |
| 2.1 | Release builds of the base and this branch, `HYALOS_TRACE=1 task-kernel run`, seed 0, 21 runs each, interleaved in two rounds: base medians 2.33 ms and 2.54 ms, ring 3 medians 2.47 ms and 2.41 ms. The largest difference is +6%, within the 20% allowed |

The start-state contract is also held by unit tests: `core_s_02_the_machine_starts_in_ring_3_with_tsd_and_iopl_3` checks the segments, `CR4`, `RFLAGS` and the empty descriptor tables against `design.md` §2, and `core_s_02_every_reachable_page_is_a_user_page_and_the_tables_are_unmapped` checks the user bit on every level, the unchanged rights and that the page tables stay unmapped.

**The probes test ring 3, not something else.** The new self-test kernel's probes were run on the base's ring 0 machine (this branch's monitor with `kvm.rs` from `9c6706b`): `RDTSC`, `RDTSCP`, clearing `CR4.TSD`, `CR0`, `LGDT`, `LIDT` and `WRMSR` all survived to the unknown request (`BadHypercall`). Only the ring 3 machine faults on them.

Verify, run locally on KVM: Verify's two builds and `run` as written (the `run` output above); the `core_s_02` filter, 16 passed; the whole `hyalos-os` workspace with `HYALOS_REQUIRE_KVM=1` and `--no-fail-fast`, after the two `attest` builds, passed 391 tests (Slice 1's 375 and these 16), none failed, ignored or skipped. CI's `fmt` and `clippy` steps for the new crates, `cargo deny` on the `hyalos-os` workspace, and `cargo xtask layout`, `lines` and `docs` pass. The root workspace is not touched.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files: modify `kvm.rs`, `main.rs`, `kernel-image/src/main.rs` | Also creates `monitor/src/ring_tests.rs` and changes `monitor/src/profile_tests.rs` | The tests need their own file, as Slice 1's do. The KVM gate now also builds the self-test kernel, into the kernel image's ignored `target-selftest` directory, so it never replaces the real image while another test reads it | n |
| `design.md` §2 lists no descriptor table values | The start state sets the GDT and IDT to base 0, limit 0 | Before, they kept KVM's reset values (limit `0xffff` at address 0), which only faulted because address 0 is unmapped. An empty table makes "no handlers" explicit and independent of the memory map. Transcripts are unchanged | n |
| S-02.3.e1 and `HLT` as evidence of privilege | On the ring 0 machine both also end in `KernelFault`: the `CR3` probe loads tables that do not map its code, and `HLT` was already reported as `KernelFault` | The other seven probes discriminate. The `CR3` probe is kept as the example states it | n |
| Verify's whole-workspace run passes on its own | It needs the two `attest` builds first, as Slice 1 found | Unchanged from Slice 1 | n |

## Decisions

- **One probe list.** The probe names and expected reasons are one `PROBES` table in `monitor/src/main.rs`, indexed by the byte that chooses each probe in the kernel image. The `attacks` command and the tests both read it, so they cannot drift. The memory probes' printed names are unchanged.
- **Probes added.** S-02.2 and S-02.3 as listed, plus `CR0` (turn write protect off) and `LIDT` beside `LGDT`, since "a control register" and "a descriptor table" each name more than one.
- **`halt` in the kernel image keeps `hlt`.** The monitor ends the run at the fault request before it, and should it not, `hlt` faults in ring 3, which also ends the run. Only its comment changed.
- **`IOPL` 3 also allows `CLI`, `STI` and `POPF` of the interrupt flag in ring 3.** The machine has no interrupt controller and no device, so nothing can be delivered. Stated here for Slice 12's limits.

## Discoveries

- **The KVM deadline does not work off the main thread.** `Ticker` arms a process-wide `SIGALRM`, and it interrupts the virtual CPU only if the signal reaches the thread running it. Under the test harness it reaches another thread, so a kernel that spins never times out: the raw `spin forever` hostile kernel hung a test until killed. The `attacks` command and `live` run the machine on the main thread, and `attacks` passes `spin forever` with `Timeout`. `live` also starts a reader thread per model call (`monitor/src/live.rs:111`) that does not block the signal; it lives only while the machine waits on that call, but this was not tested. The test leaves that one case to `attacks`. Slice 6, which moves the machine into a runner process, should deliver the deadline to the virtual CPU's thread (for example a timer with `SIGEV_THREAD_ID`, or `pthread_kill`), and could then test `Timeout` directly.
- **Fault exits.** In ring 3 every probe ends in `Shutdown` (a triple fault), never `Hlt`. Both still map to `KernelFault`, so no reason changed.
- **CPUID is untouched.** `RDRAND`, `RDPID` and `RDSEED` still run in ring 3 until Slice 3's filter, as `context.md` §2 measured.
- The self-test kernel builds in about 10 s the first time; the suite then runs in under 2 s.
