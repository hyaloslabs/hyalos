# hyalOS review package

hyalOS has not yet been independently reviewed. This package is what a reviewer needs to start one
without asking its authors: what is in scope, what it defends against, the interface a guest sees,
the limits we already know, the commit to read, and the commands to build and test it. Engaging a
reviewer and closing what they find is tracked in #491. #63 is the separate review of v1's trusted
core (`core/`), and this package does not cover it.

`cargo xtask review-package` checks this file in CI: every section below is present, the commit
under review is an ancestor of the checked-out tree, the sentence that opens this file is here, no
other line claims the code is already reviewed, and every code reference written as
`` `path.rs::name` `` resolves.

## What hyalOS is

hyalOS runs one signed WebAssembly program, the guest, over a task's inputs. The guest can ask an
approved model a bounded question and perform at most one effect, and every run is written to a
signed, hash-chained record that a customer can replay and audit alone. It is its own Cargo
workspace under `hyalos-os/` (ADR-0130), x86-64 Linux with KVM only.

```text
  KVM machine, ring 3 only          machine runner                 holder (unconfined)
  +------------------------+        Landlock + seccomp             +------------------------+
  | kernel image + wasmi   | port   +----------------------+ one   | Host rules, deadline,  |
  |   signed guest         |------->| vCPU loop, first     |------>| model calls, effect,   |
  +------------------------+ 0x1000 | parse of each request| socket| the operator's chain   |
                                    +----------------------+ pair  +------------------------+
```

| Part | Path | What a reviewer should know |
|---|---|---|
| Kernel image | `hyalos-os/kernel-image/` | `no_std`, `x86_64-unknown-none`, holds the `wasmi` interpreter. Runs in 64-bit ring 3 with no fault handlers. Its own workspace and lockfile |
| Guest | `hyalos-os/guest/` | The one committed guest, `digest.wasm`, built from `guest/src/lib.rs` |
| Monitor (`task-kernel`) | `hyalos-os/monitor/` | The holder and, re-executed as `machine-runner`, the confined runner. `kvm.rs` builds the machine, `runner.rs` is the channel and confinement, `live.rs` a live run |
| Audit and replay | `hyalos-os/audit/` | `Host`, the call limits, replay, and what an audit states (`report.rs`) |
| Verifier | `hyalos-os/verify/` | `hyalos-verify`, the customer's program |
| Profile | `hyalos-os/profile/` | The execution profiles `hyalos.profile/1` and `/2` |
| Attestation | `hyalos-os/attest/`, `hyalos-os/attest-check/` | The notary, the attestation tool and the provider canary, over TLSNotary |

The record format is `hyalos-os/RECORD.md`. The plan behind this work is
[`docs/plans/hyalos-kernel-core`](../../docs/plans/hyalos-kernel-core/README.md), and its decisions
are under `decisions/` there. The repository's own records are ADR-0130 to ADR-0135 under
`docs/adr/`.

## Scope

**Our own code.** All of `hyalos-os/`: about 9,000 lines of non-test Rust against a budget of
12,000 (`cargo xtask lines`, ADR-0133). `unsafe` is allowed only in the eleven files of
`crates/xtask/src/hyalos.rs::UNSAFE_FILES`, each use with a `SAFETY` comment (`cargo xtask
hyalos-unsafe`). The test-only code behind the monitor's `test-switches` feature and the kernel
image's `selftest` feature is in scope too, since a mistake there could leak into a release build;
`hyalos-os/monitor/tests/release_switches.rs` is the check that it does not.

**Code we did not write.** [`scope.toml`](scope.toml) gives every third-party component the claim
rests on one row per part, `use` (how hyalOS uses it) or `internals`, each with its locked version,
its upstream audit status on a stated date, `covered` or `excluded`, and a reason. `cargo xtask
review-scope` fails when a lockfile moves without its row (ADR-0135). In short:

| Component | Covered | Excluded |
|---|---|---|
| `wasmi` 2.0.3 and `wasmi_core`, `wasmi_ir`, `wasmi_collections`, `wasmparser` | Our configuration and host boundary, and the internals as far as the reviewer judges useful: the changes since the last audited versions (`0.31.0`, `0.36.0` to `0.38.0`), including 2.0.3's new upstream `unsafe` in `wasmi::Error` | Lock-only `wasmparser` versions no build uses |
| `kvm-ioctls`, `kvm-bindings` | Our use: the monitor's `unsafe` boundary | |
| `seccompiler`, `landlock` | Our use and the policy that results | |
| TLSNotary (`tlsn`, `tlsn-core`, `tlsn-attestation` at tag `v0.1.0-beta.1`) | Our use: trust settings, what we accept and what we report | Its protocol and cryptography (ADR-005) |
| `sha2`, `p256`, `k256` | Our use | Their internals |
| `rustls`, `rustls-webpki`, `webpki-roots`, `tokio-rustls`, `ring`, `rcgen`, `ureq` | Our TLS configuration, the trust anchors, and our own certificate check (`Pin` in `hyalos-os/attest/src/lib.rs`) | The crates' internals (ADR-008) |
| `bincode` | Our use: it decodes untrusted presentations | |
| Utility crates (`serde`, `tokio`, `libc` and the rest in `scope.toml`) | | Excluded, each with its reason |

A review that follows this package cannot be described as covering TLSNotary's protocol or
cryptography, or the internals of the TLS, signature and hash crates. Moving TLSNotary's pin follows
[`pin-policy.md`](pin-policy.md).

**Out of scope.** v1's trusted core (#63), the hyalOS spike rigs under `spikes/`, the host's kernel
and KVM, and the operator's machine beyond what the runner can reach.

## Threat model

From the plan's `design.md`, **Modifier: security**.

**Assets.** The operator's chain and signing path; the model and GitHub credentials the holder's
tools reach; the integrity of a recorded result; the host itself.

**Entry points.** Guest bytes; task inputs (issue titles, untrusted); the five requests the kernel
image makes through port `0x1000`; frames on the runner channel; the kernel image file; the
environment variables and arguments that select test behaviour.

**Attacker.** A guest, or a task input, that exploits `wasmi` and so controls the kernel image; from
there, code that exploits the monitor's request handling and so controls the machine runner.

| Threat | Mitigation | Test |
|---|---|---|
| A guest exploits `wasmi` and takes over the kernel | Ring 3, read-only code, NX data, no clock: `CR4.TSD` and empty descriptor tables, so any fault shuts the machine down as `KernelFault` | `hyalos-os/monitor/src/ring_tests.rs::core_s_02_2_e1_reading_the_time_counter_faults`, `::core_s_02_4_every_memory_probe_still_faults` |
| A taken-over kernel rewrites its page tables or its privileged state | Ring 3; the tables sit in unmapped pages | `hyalos-os/monitor/src/ring_tests.rs::core_s_02_3_e1_loading_other_page_tables_faults`, `::core_s_02_3_control_registers_descriptor_tables_msrs_and_hlt_fault` |
| A taken-over kernel gathers host identity or randomness | The CPUID allow-list, the host probe, replay | `hyalos-os/monitor/src/cpuid_tests.rs::core_s_03_1_the_kernel_sees_only_the_allowed_list`, `hyalos-os/monitor/src/probe_host_tests.rs::core_s_03_4_e1_a_record_whose_question_rdseed_varied_is_refused_as_diverged` |
| A hostile request exploits the handler | Bounds checks, a fixed-seed fuzzer, and the confined runner around it | `hyalos-os/monitor/src/runner_tests.rs::core_s_05_5_frame_fuzzer`, `hyalos-os/monitor/src/kvm.rs::fuzz` |
| A compromised runner lies to the holder | The holder re-checks every length against `Host`, keeps the single-effect rule and the deadline | `hyalos-os/monitor/src/runner_tests.rs::core_s_04_4_e2_a_second_effect_request_is_refused`, `::core_s_04_4_a_fuzzed_channel_never_panics_or_performs_an_unasked_effect` |
| A compromised runner attacks the host | Landlock, the seccomp allow-list, an empty environment, inherited descriptors closed | `hyalos-os/monitor/src/confine_tests.rs::core_s_04_2_e2_a_sealed_runner_that_connects_a_socket_is_killed`, `::core_s_04_2_e3_a_sealed_runner_that_starts_a_program_is_killed`, `hyalos-os/monitor/src/runner_tests.rs::core_s_04_1_e1_the_vcpu_runs_in_a_child_with_only_its_kvm_handles_and_the_channel_open` |
| A host that cannot confine the runner | Refused before the chain is written; no fallback (ADR-0132) | `hyalos-os/monitor/src/confine_tests.rs::core_s_04_3_both_confinements_hold_before_any_guest_instruction` |
| A `live` run executes a guest unconfined | A `live` run with a real repository requires the KVM kernel | `hyalos-os/monitor/src/live_tests.rs::core_s_04_5_e1_a_real_repository_without_a_kernel_is_refused_with_no_chain_link` |
| Test switches weaken a release build | Compiled out, or refused | `hyalos-os/monitor/tests/release_switches.rs::core_s_05_4_every_switch_is_named_only_in_code_built_with_the_feature`, `::core_s_05_4_e2_a_raw_kernel_image_is_refused` |

The most useful findings are a way from a guest or a task input to anything outside the runner; a
way for the runner to make the holder perform an effect the guest did not ask for, or a second one;
a result that replay accepts but the guest did not produce; and a release build in which a test
switch has an effect.

## Interface

**The guest interface, `hyalos.abi/1`.** [`ABI.md`](../ABI.md) is the contract: four imports from
module `hyalos` (`read`, `infer`, `effect`, `return`), the exports `run` and `memory`, each call's
limit, and every refusal a call can end in. Both backends, the ordinary process and the kernel image
on KVM, are held to it by `hyalos-os/monitor/src/abi_tests.rs::core_s_01_1_every_call_refuses_as_abi_md_states_on_both_backends`,
and a change to a call fails `::core_s_01_2_e2_a_limit_raised_without_a_version_change_fails` until
the interface gets a new name (ADR-004 of the plan). The record does not carry the interface's
name. The interpreter's settings are the execution profile (`hyalos.profile/2` for new runs,
`RECORD.md`).

**The machine.** `hyalos-os/monitor/src/kvm.rs::start_state`: one virtual CPU, 32 MiB, 64-bit ring
3 (code selector 11, data 19), `CR4` with `PAE` and `TSD`, `RFLAGS` with `IOPL` 3 and interrupts
off, empty GDT and IDT, the User bit on every page entry the kernel may reach, and the page tables
in pages it cannot. `IA32_APIC_BASE` is written to 0 before the start, so KVM does not add the
`APIC` bit to the list below.

**The CPUID list.** `hyalos-os/monitor/src/kvm.rs::allowed_cpuid` gives four leaves, each bit only
where the host has it. Every other bit and leaf reads as zero, or as leaf 1's filtered data where
KVM answers a leaf past the highest:

| Leaf | Exposed |
|---|---|
| `0` | Highest basic leaf `1`; vendor string zeroed |
| `1` | `PAE` only (`EAX` and `EBX`, family, model and APIC ID, are zero) |
| `0x80000000` | Highest extended leaf `0x80000001` |
| `0x80000001` | Long mode and `NX` only |

So the `RDRAND`, `RDSEED`, `RDTSCP` and `RDPID` bits are clear, and no leaf holds host data. Whether
the instructions then fault is the host's (see **Limits**).

**The runner channel, `hyalos.runner/1`.** Internal, with its own version byte
(`hyalos-os/monitor/src/runner.rs::VERSION`). A frame is a 4-byte little-endian length, a version
byte, a type byte (`Boot`, Reply, `Input`, `PreInfer`, `PreEffect`, `Infer`, `Effect`,
`Finished`, `Failed`) and its fields. Limits: `Boot` 8 MiB (`::BOOT_LIMIT`), a reply 16 MiB and 64
bytes (`::REPLY_LIMIT`), a request 64 KiB and 64 bytes (`::REQUEST_LIMIT`). A reason is its index in
the closed table `::REASONS`. A frame over its limit, of an unknown type or out of turn ends the run
as `KernelFault`.

**The seccomp allow-list.** `hyalos-os/monitor/src/runner.rs::ALLOWED`, installed after the machine
is built and before its first instruction (ADR-0132). Anything else, or any of these outside its
restriction, kills the runner with `SIGSYS`:

| System call | Restriction |
|---|---|
| `ioctl` | Only `KVM_RUN`, only on the virtual CPU |
| `recvfrom` | Only the channel |
| `sendto` | Only the channel, with no address |
| `write` | Only standard error, only under `--trace`, which a release build refuses |
| `mmap` | Only anonymous, never executable |
| `munmap`, `brk` | None |
| `clock_gettime` | Only `CLOCK_MONOTONIC` |
| `close` | None |
| `exit_group` | None |

**Landlock.** `hyalos-os/monitor/src/runner.rs::landlock`, applied after `/dev/kvm` is opened and
before any byte of the run arrives: every file right of ABI 1 is required, and the file, TCP and
scope rights up to ABI 6 are taken where the kernel has them, with no rule granting any back.

**The host probe.** `task-kernel probe-host` prints `blocked` or `reachable` for `RDTSC`,
`RDTSCP`, `RDRAND`, `RDSEED` and `RDPID`, each run on a fresh machine with the real start state and
CPUID list, and exits 0 when all are blocked, 1 when any is reachable and 2 when it could not test
(`hyalos-os/monitor/src/probe_host.rs::INSTRUCTIONS`).

## Limits

Each of these is known and accepted for now. A finding that one is worse than stated is in scope.

**Trust and scope.**

- KVM and the host's Linux kernel are trusted. So are the CPU and its microcode.
- Side channels are out of scope, including timing a taken-over kernel controls, such as how long it
  takes to make a request.
- TLSNotary's protocol and cryptography are excluded (ADR-005), and so are the internals of
  `rustls`, `rustls-webpki`, `tokio-rustls`, `ring`, `rcgen`, `ureq`, `sha2`, `p256` and `k256`
  (ADR-008), and the excluded utility crates (`scope.toml`).
- `attest`'s `forge-ticket` and `canary-test-hooks` features are test-only and off by default. So
  are the monitor's `test-switches` and the kernel image's `selftest`. The reviewed binaries are
  built without them, and the `attacks` command exists only in a test build.
- No `auditor` findings are linked, since none have been run on this code.

**Randomness and time in a taken-over kernel.**
hyalOS does not claim that a taken-over kernel has no hardware randomness on every host.
The CPUID bits are clear everywhere, but whether `RDRAND`,
`RDSEED` and `RDPID` then fault is up to the host's KVM: AMD's SVM cannot trap `RDRAND` or `RDPID`
at all, and `RDSEED` stays reachable on some Intel hosts (ADR-002 and ADR-009 of the plan). On such
a host a taken-over kernel can read unpredictable values and the processor ID. It still cannot
change a verified result: replay refuses a record whose requests, effects or result differ from
what the guest gives (`hyalos-os/monitor/src/probe_host_tests.rs::core_s_03_4_e1_a_record_whose_question_rdseed_varied_is_refused_as_diverged`).
A reachable instruction does not refuse a run. `RDTSC` and `RDTSCP` faulted on every host measured.
Run `task-kernel probe-host` on the host you review. Measured so far:

| Host | `RDRAND` | `RDSEED` | `RDPID` | Where |
|---|---|---|---|---|
| Intel, nested KVM under WSL2 (development host) | blocked | reachable | blocked | This package's dry run, 2026-11-25 |
| `GenuineIntel` Xeon 6975P-C, `kvm_intel` (CI) | blocked | blocked | blocked | CI runs `45547863519`, `45551214113` |
| `GenuineIntel` Xeon Platinum 8573C (CI) | blocked | reachable | blocked | CI run `45556140227` |
| `AuthenticAMD` EPYC 7763, `kvm_amd` (CI) | reachable | reachable | reachable | CI run `45552923599` |

No bare-metal host has been measured.

**The machine.**

- `IOPL` 3 lets the ring 3 kernel run `CLI`, `STI` and `POPF` on the interrupt flag, and use any
  port. Every port exits to the monitor, which answers only `0x1000`. The machine has no interrupt
  controller and no device, so nothing can be delivered.
- The KVM backend refuses a guest over 1 MiB, and only it has a deadline (`Timeout`); the process
  backend has neither (`ABI.md`, "Not part of the interface").
- No record names or hashes the kernel image, and a record does not name the interface version
  (non-goals of the plan).

**The runner.**

- A runner killed by its filter may leave a core dump wherever the host's `core_pattern` sends it,
  and its memory holds the task's inputs (ADR-0132). Setting `RLIMIT_CORE` to 0 in the runner would
  stop it and keep `/proc/<runner>` readable; #492 tracks it.
- Landlock's rights after ABI 1 are best effort. The development host has ABI 3, so TCP (ABI 4),
  `IOCTL_DEV` (ABI 5) and the scopes (ABI 6) are not enforced there, and no test records which ABI
  CI's kernel enforces, so `IOCTL_DEV` is untested in CI. The seccomp list blocks sockets, `ioctl`
  other than `KVM_RUN`, and signals on its own.
- The allow-list was measured on the development host and CI's runners, on the pinned toolchain. A
  host, C library or standard library that needs another system call fails closed until it is
  reviewed and added.
- Splitting the runner out made a run's whole start slower: about 7.0 ms from the run's start to the
  first request, against 3.8 ms before (+85%), from spawning the runner, sending the 2 MB kernel over
  the socket and each request crossing two processes. The plan's limit, boot to first call, stayed
  within its 20% (about 2.0 ms against 2.1 ms).

**The checks.**

- `cargo xtask hyalos-unsafe` is textual: it does not parse Rust, so `unsafe` in a string or a block
  comment fails it, and `unsafe` that a macro from another crate expands to is not seen by it.
  Third-party `unsafe`, such as `wasmi` 2.0.3's in `wasmi::Error`, is reviewed through `scope.toml`
  instead (ADR-0133, ADR-0135).
- The audit citations in `scope.toml` are a web search's result on the dates the rows give, not
  proof that no audit exists.
- The presentation differential the pin policy requires before TLSNotary's pin moves does not exist
  yet. The first move adds it.
- `digest.wasm` is reproducible from `guest/src/lib.rs` on the pinned toolchain, but no check holds
  that.
- `aes` 0.9.0 and `der` 0.8.0, under TLSNotary, are yanked on crates.io with no advisory. They move
  only with TLSNotary's pin.

**`wasmi` 2.0.3.** GHSA-qrr7-2g7f-r9rg named `wasmi` 2.0.2, so new runs use `hyalos.profile/2` on
2.0.3, and profile 1's records replay on 2.0.3 through the checked-equal pair (ADR-0134). A customer
must authorise the profile 2 task identity for new runs. Customers get the fix in `hyalos-verify`
only with a new verifier release, which is the operator's decision (ADR-0131).

**Open issues.** Known, tracked, and not fixed by this package:

- #490: the in-process backend and the audit's replay take `wasmi`'s default `wat` feature, so they
  accept a WebAssembly text guest that the kernel image refuses.
- #484: CI never runs the audit's no-stand-in test without `test-trust`.
- #487: the trusted core's line counter, in `crates/xtask`, has no guard against code after
  `#[cfg(test)]`. hyalOS's own counter has one.
- #412: the scope this package carries; it stays open until the review it asks for exists.
- `ledger::fixture_tests::local_4_2_generate` sometimes fails on a timing-dependent message (`answer
  6` or `answer 7`). It has no issue; a rerun passes.

## Commit under review

`7a6e6481c0f07939c295f0baa87412d24f16af04`

This is the merge-base of the pull request that adds this package (#468's), since a file cannot name
the commit that adds it. The code under review is that commit. The pull request adds only this file and
its check (`cargo xtask review-package` and its CI step), with the plan's notes, so on the commit that
merges it this prints nothing:

```bash
git diff --stat 7a6e6481c0f07939c295f0baa87412d24f16af04 HEAD -- hyalos-os ':(exclude)hyalos-os/review/README.md'
```

## Commands

Run them from a clean clone, at the repository root, on x86-64 Linux. The toolchain is pinned in
`rust-toolchain.toml` (1.99.1), and the first build fetches and compiles every dependency, which
takes several minutes. Tests that need KVM or Landlock skip without them unless the matching
variable is set, and a skip is not a pass.

**Common setup.**

```bash
git clone https://github.com/hyaloslabs/hyalos.git && cd hyalos
rustup toolchain install
rustup target add x86_64-unknown-none wasm32-unknown-unknown
# The attestation tests run these two programs, so they are built first.
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
```

**On a KVM host.** You need read and write access to `/dev/kvm`, and a kernel with Landlock and
seccomp. With both variables set, a test that cannot reach KVM or Landlock fails instead of
skipping.

```bash
HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches
HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --test release_switches
cargo run --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel -- fuzz hyalos-os/evidence/fixtures/issues.tsv 100000
cargo run --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel -- probe-host
```

The first two must pass. The fuzzer ends with exit 0. The probe prints five lines and exits 0 or 1;
exit 2 means it could not test.

**On a host without KVM.** The same tests, without the variables, keeping their output so each
skipped test can be seen with its reason:

```bash
set -o pipefail
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches -- --nocapture 2>&1 | tee test.log
grep '^skipped:' test.log
cargo run --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel -- probe-host
```

The tests pass, and each KVM test prints `skipped: <test> needs KVM` (or `needs KVM and Landlock`,
or `needs Landlock`). The probe exits 2 and names the cause.

**The repository's checks** (either host):

```bash
cargo xtask lines
cargo xtask hyalos-unsafe
cargo xtask review-scope
cargo xtask review-package
cargo fmt --manifest-path hyalos-os/Cargo.toml --all --check
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml --workspace --all-targets --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches -- -D warnings
# If cargo-deny is installed:
cargo deny --manifest-path hyalos-os/Cargo.toml --config hyalos-os/deny.toml check
```

`.github/workflows/ci.yml`'s `hyalos-os` and `lint` jobs run all of these on every change.

## Findings

**Reporting.** Report a finding privately, as [`SECURITY.md`](../../SECURITY.md) says: on the
repository's **Security** tab, **Report a vulnerability**. Never in a public issue, discussion or
pull request. Name the commit above, the part of the threat model it breaks, and steps or a test that
show it. Never include a real secret.

**Closing.** Each finding is closed in one of two ways, and the finding cites which:

- **Fixed**: by a commit on `main` that fixes it, with a test that fails before the fix. The finding
  cites the fixing commit.
- **Accepted**: by an ADR under `docs/adr/` that states the limit and why it is accepted, and adds it
  to **Limits** here. The finding cites the ADR.

No release or document may claim this review until every finding is closed one of those ways, and
then only for the commit and the scope above. #491 tracks the engagement and
its findings.
