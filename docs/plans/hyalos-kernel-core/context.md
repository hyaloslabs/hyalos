# Context — what exists today

Every citation below was checked on 2026-11-23 at commit `63a75ec`, after the verifier pack's Slices 1 to 3 (#443, #444, #445) merged. Preflight (README rule 2) means re-checking the ones your slice touches.

**Older citations.** ADR-004 cites `spikes/task-kernel/ABI.md`, now `hyalos-os/ABI.md`. ADR-001's last bullet predates the 64-bit ring 3 run recorded in §2. Accepted ADRs are not edited.

**Line counts.** ADR-007's figures are an estimate from 2026-11-23 and omit `hyalos-os/profile`. The check in Slice 10 is what counts.

**Moving code.** The verifier's Slices 4 to 7 (#446 to #449) are still open and may change `hyalos-os/monitor/src/main.rs` and the new `hyalos-os/audit` and `hyalos-os/verify` crates. Find symbols in that file by name when a line has moved.

## 1. Repository shape

- **Two workspaces.** The root workspace holds `core/*` and `crates/*` (`Cargo.toml:3`) and forbids `unsafe` (`Cargo.toml:14`). hyalOS is its own workspace by parent ADR-001, with the kernel image and guest built apart.
- **The monitor** is an ordinary Linux program. It boots the kernel image on KVM, gives it one virtual CPU and 32 MiB of memory, and answers its requests (`hyalos-os/monitor/src/kvm.rs:1`, `:14`).
- **The kernel image** is a `no_std` program for `x86_64-unknown-none` holding the `wasmi` interpreter (`hyalos-os/kernel-image/src/main.rs:6`, `hyalos-os/kernel-image/.cargo/config.toml`).
- **Two backends.** A guest runs either in an ordinary process or on KVM, chosen by `HYALOS_KERNEL` (`hyalos-os/monitor/src/main.rs:327`).
- **The audit does not run the KVM backend** and opens no kernel file (`hyalos-os/monitor/src/main.rs:325`). No record names or hashes the kernel image.

## 2. What this work changes

### The machine

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/monitor/src/kvm.rs` | 263–264 | Gives the guest KVM's whole supported CPUID list | Changed: an allowed list |
| `hyalos-os/monitor/src/kvm.rs` | 268–273 | Starts the kernel in ring 0, with only `CR4.PAE` set | Changed: ring 3 with `CR4.TSD` |
| `hyalos-os/monitor/src/kvm.rs` | 144–174 | Builds page tables with no user bit, and places them in the unmapped first megabyte | Changed: user pages |
| `hyalos-os/monitor/src/kvm.rs` | 112–119 | Accepts a raw, non-ELF image and maps it writable and executable, for hostile-kernel tests | Changed: test builds only |
| `hyalos-os/monitor/src/kvm.rs` | 181–236 | Checks and answers the five requests | Kept. Moves behind the confinement channel |
| `hyalos-os/monitor/src/kvm.rs` | 289 | Treats `Hlt` and `Shutdown` as a kernel fault | Kept |
| `hyalos-os/monitor/src/kvm.rs` | 329–365 | Fuzzes the request handler | Kept. Runs in CI |
| `hyalos-os/kernel-image/src/main.rs` | 95–109 | Leaves through `out` on port `0x1000`, and halts after a fault | Changed where ring 3 requires it |
| `hyalos-os/kernel-image/src/main.rs` | 260–278 | Self-test probes behind the `selftest` feature | Extended |

### The monitor process

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/monitor/src/live.rs` | 176 | Starts the attestation tool | Kept, in the unconfined parent |
| `hyalos-os/monitor/src/live.rs` | 202 | Starts `gh` for the one notification | Kept, in the unconfined parent |
| `hyalos-os/monitor/src/main.rs` | 351–395 | The `live` command runs the task and writes the chain in one process | Changed: the machine runs in a confined child |
| `hyalos-os/monitor/src/main.rs` | 198 | `HYALOS_SELFTEST_KERNEL` chooses a kernel file for the `attacks` command | Test builds only |
| `hyalos-os/monitor/src/kvm.rs` | 41, 240 | `HYALOS_HUGE` and `HYALOS_TRACE` change behaviour through the environment | Test builds only |

### The third-party base

| Component | Pin | Where |
|---|---|---|
| `wasmi` | `=2.0.2` | `hyalos-os/monitor/Cargo.toml`, `hyalos-os/kernel-image/Cargo.toml` |
| `kvm-ioctls`, `kvm-bindings` | `=0.25.1`, `=0.14.2` | `hyalos-os/monitor/Cargo.toml` |
| TLSNotary (`tlsn`) | tag `v0.1.0-beta.1` | `hyalos-os/attest/Cargo.toml:12` |

**Upstream audit status, searched on 2026-11-23.** `wasmi` has public audits of `0.31.0` (SRLabs, 2023) and `0.36.0`–`0.38.0` ([Runtime Verification, 2024](https://runtimeverification.com/public-report/wasmi)). Neither covers the pinned `2.0.2`. One search found no published code audit of TLSNotary's `tlsn`, which is not proof that none exists. [A 2024 paper](https://arxiv.org/abs/2409.17670) reviews its protocol.

### Measured on 2026-11-23

A throwaway program booted one KVM machine per case on this repository's development host (Intel, WSL2, Linux 6.6.87, so KVM is nested under Hyper-V). Each ran one instruction in 32-bit protected mode, so this table has not been repeated in long mode or on bare metal. Slice 4's probe is the first 64-bit measurement of it.

| Instruction | Ring 0 with `CR4.TSD` | Ring 3 with `CR4.TSD` | Ring 3 with `CR4.TSD` and the CPUID bits cleared |
|---|---|---|---|
| `RDTSC` | runs | faults | faults |
| `RDTSCP` | runs | faults | faults |
| `RDRAND` | runs | runs | faults |
| `RDPID` | runs | runs | faults |
| `RDSEED` | runs | runs | **runs, returning changing values** |
| Clear `CR4.TSD`, then `RDTSC` | runs | faults | faults |

KVM is written to intercept `RDSEED` when a guest's CPUID lacks it, so this result may be specific to the nested host. It has not been explained.

A second throwaway run patched only the monitor's start state in a copy of `02f66d1`: the User bit on page entries, ring 3 segments, `CR4.TSD` and `IOPL` 3. The kernel image was unchanged. In 64-bit mode, transcripts for seeds `0` and `7` were byte-identical to ring 0, all 51 `attacks` cases passed (14 memory and 18 hostile-kernel), and median boot to first call over 21 runs was 2.17 ms for both. The CPUID filter was not part of this run. `hyalos-os/guest/digest.wasm` is the only committed guest.

**A `live` run can skip the machine.** With `HYALOS_KERNEL` unset, `load_backend` returns the process backend (`hyalos-os/monitor/src/main.rs:327`), and the `live` command uses it (`:352`), so the guest runs inside the process that reaches the chain and tools.

## 3. Shared mechanics to reuse

- Landlock confinement that fails closed and reports its enforcement level: `core/hyalos/src/confine.rs:67`, availability check at `:99`. The rule that nothing runs without it: `docs/adr/0121-no-broker-runs-without-landlock.md`.
- The hostile-kernel attack list and its expected reasons: `hyalos-os/monitor/src/main.rs:150` (`hostile_kernels`), `:181`, `:192`.
- The self-test kernel's probe pattern, one attack chosen by the first input byte: `hyalos-os/kernel-image/src/main.rs:261`.
- Replay, which compares a fresh run with the record: `hyalos-os/monitor/src/main.rs:105`.
- Line budgets: `crates/xtask/src/main.rs:17`, `:21`, `:349`.
- Dependency policy: `deny.toml:50-54`, and the workspace copy `hyalos-os/deny.toml` the verifier's Slice 1 adds.
- The report's stated limits: `hyalos-os/audit/src/report.rs:6`, `:79–100`.
- The execution profile `hyalos.profile/1`: `docs/plans/hyalos-kernel-verifier/design.md`, §2. This pack consumes it and does not change it (verifier ADR-007).

## 4. How verification works here

Commands from the verifier pack's Slice 1, run after #443 merges:

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust
cargo deny --manifest-path hyalos-os/Cargo.toml check --config hyalos-os/deny.toml
cargo xtask layout
cargo xtask lines
```

Expected: all pass. Tests that need KVM must run on a host with `/dev/kvm`, and the CI variable that requires them is added by the verifier pack (`HYALOS_REQUIRE_KVM`). A skip is not a pass.

## 5. Glossary

| Term | Meaning |
|---|---|
| Guest | The signed WebAssembly program of a task |
| Kernel image | The bare-metal program holding the interpreter, which runs inside the KVM machine |
| Monitor | The Linux program that boots the kernel image and answers its requests |
| Taken-over kernel | A kernel image running code an attacker chose, for example through an interpreter bug |
| Guest interface | The four calls a guest may import: `read`, `infer`, `effect` and `return` |
| Request | One of the five messages from the kernel image to the monitor |
| Execution profile | The interpreter settings and limits a record names, defined by the verifier pack |
| Machine runner | The confined child process that owns the KVM machine |
| Holder | The unconfined parent process that performs model calls, the effect and the chain |
| Review scope | The list of components the independent review covers or excludes, each with a reason |
| Review package | What a reviewer receives: scope, threat model, build and test commands, and known limits |
