# Slice 3 progress

- **Status:** done
- **Issue:** #459
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-24

## Preflight

Checked at base commit `6dbf81c` (Slice 2 merged). Symbols were found by name, as `context.md` asks.

| Citation | Result |
|---|---|
| `hyalos-os/monitor/src/kvm.rs:263–264`, the whole supported CPUID list (also plan.md's **Pattern to follow**) | Differs by line only: `get_supported_cpuid` and `set_cpuid2` at `:295–296`, moved by Slice 2. Behaviour as described |
| `hyalos-os/monitor/src/kvm.rs:268–273`, the start state | Holds as Slice 2 left it: `start_state` at `:191`, ring 3 with `CR4.PAE` and `TSD`, `EFER` long mode and `NX` |
| `hyalos-os/monitor/src/kvm.rs:289`, `Hlt` and `Shutdown` are `KernelFault` | Holds (now in `run`'s loop) |
| `hyalos-os/kernel-image/src/main.rs:260–278`, the self-test probes | Holds: `probe` with bytes 1–16, as Slice 2 left it |
| `hyalos-os/monitor/src/main.rs`, `PROBES` and `run_attacks` (Slice 2) | Holds |
| `hyalos-os/monitor/src/profile_tests.rs:65`, the KVM gate, and `selftest_kvm_backend` | Holds |
| `design.md` §2, **CPUID allow-list** | Holds: nothing in the code contradicted the table |
| `context.md` §2, `RDRAND` and `RDPID` run in ring 3 until their CPUID bits are cleared | Repeated in 64-bit ring 3: see the counter-check below |

No difference touches this slice's contracts.

**Base, recorded before any edit** (kept outside the repository, as the shared contract asks): Slice 2's commands at `6dbf81c` gave `ok: 40 inputs, 40 inferences, 1 effect, fuel 29845` (seed 0) and `fuel 30886` (seed 7), SHA-256 `24d971b2…46a5` and `0c7b1259…cbc2`, the hashes Slice 2 pinned.

## Acceptance

Unit tests are at the end of `hyalos-os/monitor/src/kvm.rs`, on the pure function `allowed_cpuid`, fed a hand-built host that offers every bit of every leaf with a vendor and brand string. KVM tests are in `hyalos-os/monitor/src/cpuid_tests.rs`: the self-test kernel reads CPUID from inside the machine and returns the words as the run's result, so they check what the kernel really sees. They ran locally on KVM with `HYALOS_REQUIRE_KVM=1`, so none could skip. The `core_s_03` filter runs 12 tests, all pass.

| ID | How it was shown |
|---|---|
| S-03.1 | Unit: `core_s_03_1_the_list_holds_only_the_allowed_leaves_and_bits` (four leaves, the two highest leaf numbers, `PAE`, long mode and `NX`, nothing else) and `core_s_03_1_a_bit_the_host_lacks_is_not_offered`. KVM: `core_s_03_1_the_kernel_sees_only_the_allowed_list` reads leaves `0`, `1`, `4`, `7`, `0xb`, `0xd`, `0x4000_0000–1`, `0x8000_0000–4` and `0x8000_0008` in the machine: leaf `0` is `[1, 0, 0, 0]`, leaf `1` is `PAE` only, `0x8000_0001` only long mode and `NX`, every other leaf zero or leaf `1`'s filtered data; the `RDRAND`, `RDSEED`, `RDTSCP` and `RDPID` bits are clear; and no non-zero word equals a word of the host's own vendor, signature or brand string, read outside the machine |
| S-03.1.e1 | Unit: `core_s_03_1_e1_leaf_1_has_no_rdrand_bit` (the host has it, the list does not). KVM: `core_s_03_1_e1_leaf_1_shows_no_rdrand` |
| S-03.1.e2 | Unit: `core_s_03_1_e2_no_brand_leaf_and_no_host_name` (no `0x8000_0002–4` entry, and no word of the host's vendor or brand anywhere in the list). KVM: `core_s_03_1_e2_the_brand_leaves_hold_no_host_name`: the brand leaves read as leaf `1`'s filtered data, bytes `0` and `0x40`, spelling nothing (see Deviations) |
| S-03.2 | Host-dependent under ADR-009. `core_s_03_2_rdrand_and_rdpid_fault_where_the_host_traps_them`, on the self-test kernel's new probes, bytes 17 and 18. Proven where KVM traps them: on an Intel CPU with `kvm_intel` the test requires `KernelFault`, and on the development host (Intel Core Ultra 9 185H, WSL2) both fault. On AMD with `kvm_amd` it accepts that they run; on any other host it accepts either ending and claims nothing. Any other ending fails everywhere. The judgement is unit-tested without KVM: `core_s_03_2_only_intel_kvm_is_claimed_to_trap` and `core_s_03_2_a_run_is_reachable_only_where_the_host_cannot_trap_it`. Each test writes the host's vendor, brand and KVM module past the harness's capture, so CI's log names the runner's CPU |
| S-03.2.e1 | `core_s_03_2_e1_rdrand_faults_where_the_host_traps_it`: `KernelFault` on the development host, as above elsewhere |
| S-03.2.e2 | `core_s_03_2_e2_rdpid_faults_where_the_host_traps_it`: `KernelFault` on the development host, as above elsewhere |

**The tests discriminate.** With this branch's monitor changed back to the host's whole supported list (and the `APIC` base left enabled), five of the six KVM tests fail: leaf `0` shows `GenuineIntel` and highest leaf 28, leaf `1` shows `RDRAND`, and `RDRAND` and `RDPID` both reach the unknown request (`BadHypercall`), so they ran. The sixth, `core_s_03_1_e2_the_brand_leaves_hold_no_host_name`, passes there too: this host's KVM already gives no brand leaves (see Discoveries). The unit test for e2 does discriminate, since it gives the filter a host with a brand string. After ADR-009, with the filter offering `RDRAND` again on this Intel host, `core_s_03_2_rdrand_and_rdpid_fault_where_the_host_traps_them`, its e1 and `core_s_02_every_probe_ends_as_listed` fail, so the host-dependent tests still require the fault where the host traps it.

**Verify**, run locally on KVM:

- `HYALOS_REQUIRE_KVM=1 cargo test … core_s_03`: 12 passed (10 before ADR-009, and its two unit tests).
- `HYALOS_REQUIRE_KVM=1 cargo test --release --locked --no-fail-fast --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust`, after CI's two `attest` builds: 401 tests (Slice 2's 391 and these 10), none ignored or skipped. 400 passed; `ledger::fixture_tests::local_4_2_generate` failed under the parallel load, the flake Slice 2's notes name, and passed when rerun alone. The same command was run again after review, on the final code (the `unsafe` around `__cpuid_count` removed, the `SAFETY` comment on the report's write added), with the same result: 400 passed, none ignored or skipped, and the flake passed alone.
- After ADR-009, the same whole-workspace command on the final code: 403 tests (401 and the two new unit tests), all passed, none ignored or skipped; the flake did not occur. CI's `fmt`, `clippy` and `cargo deny` steps and `cargo xtask layout`, `lines` and `docs` pass again.
- CI's other `hyalos-os` steps: `fmt` and `clippy` on the new crates and `cargo deny` pass (local `cargo-deny` 0.20.2 has no `--config`, so it found `hyalos-os/deny.toml` by itself; no dependency changed). `cargo xtask layout`, `lines` and `docs` pass. The root workspace is not touched.
- Dry run, the Slice 2 differential on this branch: `task-kernel run` printed `fuel 29845` (seed 0) and `ok: 40 inputs, 40 inferences, 1 effect, fuel 30886` (seed 7), and `cmp` found both transcripts byte-identical to the base's. `core_s_02_1_e1` also pins them.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Leaf `1` exposes `PAE`, plus any bit KVM needs for the start state | The monitor also writes `IA32_APIC_BASE` to `0` before the machine starts | KVM sets leaf `1`'s `APIC` bit at run time from the APIC base, which it enables by default, so with the list alone the kernel saw `PAE` and `APIC`. The machine has no interrupt controller, so disabling the base changes nothing it can use, and the transcripts are unchanged. KVM needed no bit beyond `PAE`, long mode and `NX` | n |
| S-03.1.e2: the brand leaves hold zeros | They hold leaf `1`'s filtered data, which is `PAE` alone (`0x40` in one word) | They are past the highest extended leaf, so KVM answers them with the highest basic leaf, as `design.md` §2 allows ("or as the highest basic leaf's filtered data"). No host data either way | n |
| S-03.2: `RDRAND` and `RDPID` fault on every host | They fault only where KVM can trap them: Intel (VMX). On CI's runner both ran and reached `BadHypercall` (run `45436579840`) | AMD's SVM has no intercept for either. The developer chose to make them host-dependent like `RDSEED` (ADR-009, amending ADR-002); the tests require the fault on Intel with `kvm_intel` and accept a run elsewhere. `PROBES` keeps `KernelFault` for both, and `HOST_DEPENDENT` and `judge` in `monitor/src/main.rs` decide, also for the `attacks` command and Slice 2's `core_s_02_every_probe_ends_as_listed` | y |
| Files: modify `kvm.rs` and `kernel-image/src/main.rs` | Also creates `monitor/src/cpuid_tests.rs` and changes `monitor/src/main.rs` (`PROBES` and the test module) | The probes join Slice 2's `PROBES` table, and the KVM tests need their own file, as Slice 2's do | n |

## Decisions

- **The list is built from constants and masked by what the host supports.** A host without `PAE`, long mode or `NX` gets a list without it, and KVM then refuses the start state (`NoKvm`), rather than the monitor offering a bit the host lacks.
- **Leaf `1`'s `EAX` and `EBX` are zero.** They hold the family, model and stepping and the APIC ID, which name the host.
- **A CPUID report, not only bits on the monitor's side.** The self-test kernel gets input byte `0x80`, outside `PROBES`, which reads 14 leaves and ends the run with them as its result. It is not on the `attacks` list, since that list checks how a run ends, not what it returns.
- **`RDRAND` and `RDPID` are host-dependent (developer, on PR #478).** This slice was blocked under README rule 6: on CI's KVM host (run `45436579840`, commit `548368d`) S-03.1 held but `RDRAND` and `RDPID` ran in ring 3 and ended with `BadHypercall`, failing the three S-03.2 tests and Slice 2's `core_s_02_every_probe_ends_as_listed`. The question was whether to make them host-dependent or to refuse KVM runs on hosts that cannot trap them. The developer chose the first: like `RDSEED` under ADR-002, the host probe reports them, runs are allowed and the limit is stated. Recorded as ADR-009, which amends ADR-002 (its Status line now points to it).
- **The trap condition is the CPU vendor with the KVM module that matches it.** `traps_randomness` claims a trap only for `GenuineIntel` with `/sys/module/kvm_intel`, knows `AuthenticAMD` or `HygonGenuine` with `kvm_amd` cannot trap, and claims nothing otherwise, so a host it cannot read never counts as proof. `/proc/cpuinfo`'s `vmx flags` was considered and rejected: the kernel hides `RDRAND` exiting from it, and the development host lists neither that nor `RDTSCP`. On the `attacks` command, a run on a host not known to trap prints `REACHABLE`, not `PASS`, and does not fail it.
- **Probe names.** The two probes are `randomness: hardware randomness (RDRAND)` and `randomness: read the processor ID (RDPID)`, so `attacks` with the self-test kernel prints them with the rest.

## Discoveries

- **KVM gives no brand leaves on this host.** `KVM_GET_SUPPORTED_CPUID` on the development host (Intel, WSL2) has no `0x8000_0002–4` entries, so even the unfiltered machine read zeros there. The vendor string, family and model, the feature bits and the hypervisor leaves were all the host's before this slice.
- **`RDSEED`'s bit is clear now**, as S-03.1 asks, but this slice adds no `RDSEED` probe. Whether it still runs is Slice 4's host probe (ADR-002).
- **For Slice 4: the host probe must report `RDRAND` and `RDPID` from a measurement, alongside `RDSEED`** (ADR-009). `design.md` §2 already lists all five instructions. This slice's vendor check is a prediction; the probe's measurement should replace it in the tests once it exists, and the self-test kernel's bytes 17 and 18 already run both instructions.
- **For Slice 12: state the limit.** The package's Limits section and `design.md`'s **Limits to state in the package** name only `RDSEED` (ADR-002). `RDRAND` and `RDPID` may be reachable too, on AMD hosts at least (ADR-009), so the package must not claim a taken-over kernel has no hardware randomness or processor ID on every host.
- **The `APIC` base.** Slice 4's probe, Slice 6's runner and the review package's interface section should keep the `IA32_APIC_BASE` write with the CPUID list: without it the list the kernel sees is not the one the monitor sets.
- **Cold start** was not measured: the slice adds one `KVM_SET_MSRS` call and a smaller CPUID table. Slice 6 compares with the Slice 2 baseline.
