# Context — what exists today

Every citation below was checked on 2026-11-25 at commit `069f31c`. Preflight (README rule 2) means
re-checking the ones your slice touches. Paths are from the repository root.

## 1. Repository shape

- **hyalOS is its own Cargo workspace** in `hyalos-os/` (ADR-0130), with members `monitor`
  (package `task-kernel`), `attest`, `attest-check`, `profile`, `audit` (package `hyalos-audit`) and
  `verify` (`hyalos-os/Cargo.toml:1-5`). `kernel-image` and `guest` are excluded and keep their own
  manifests and lockfiles, because the kernel image needs `panic = "abort"` and the guest builds for
  WebAssembly (`hyalos-os/Cargo.toml:5-6`). Nothing in `core/` depends on it.
- **Two backends run one guest.** The process backend runs the guest under `wasmi` in the monitor's
  own process (`hyalos-os/audit/src/replay.rs:300`). The KVM backend boots the kernel image, which
  runs the guest under its own `wasmi` (`hyalos-os/kernel-image/src/main.rs:248`), inside a machine
  owned by a confined machine runner process (`hyalos-os/monitor/src/runner.rs`, ADR-0132). The
  monitor chooses KVM when `HYALOS_KERNEL` names a kernel image (`hyalos-os/monitor/src/main.rs:682-689`).
- **Every host call is answered by one `Host`.** `hyalos_audit::Host` holds the inputs, answers and
  effects and owns the call checks (`hyalos-os/audit/src/replay.rs:94`). The process backend calls it
  from its linker (`hyalos-os/audit/src/replay.rs:206`); the KVM backend's holder calls the same
  methods through the `Calls` trait (`hyalos-os/monitor/src/kvm.rs:328-355`) when the runner relays
  a request (`hyalos-os/monitor/src/runner.rs:400-407`). A check added to `Host` therefore holds on
  both backends.
- **The guest interface** is `hyalos.abi/1` in `hyalos-os/ABI.md`: four imports from module `hyalos`
  (`read`, `infer`, `effect`, `return`), a `run` export and a `memory` export, every refusal named.
  Its conformance suite runs every case on both backends (`hyalos-os/monitor/src/abi_tests.rs:161`).
- **Execution profiles** fix the interpreter settings and limits a run and its replay depend on
  (`hyalos-os/profile/src/lib.rs:11-33`, ADR-0134, parent ADR-002). A changed profile is a new name;
  old ones stay.
- **The record** is a `Transcript` (`hyalos-os/audit/src/record.rs:105`), version 4 for new runs
  (`hyalos-os/audit/src/record.rs:17`), naming its profile and a framed task identity.
- **Budget and `unsafe`.** hyalOS's non-test Rust may not pass 12,000 lines
  (`crates/xtask/src/hyalos.rs:14`); 9,042 were counted at this commit (`cargo xtask lines`). `unsafe`
  is allowed only in the files on `UNSAFE_FILES` (`crates/xtask/src/hyalos.rs:18`, ADR-0133).
- **`spikes/`** holds rigs outside the workspaces and outside trusted core (`spikes/72`, `spikes/73`,
  `spikes/92`). `spikes/task-kernel`, which #438 and the parent brief cite, no longer exists: core
  moved it into `hyalos-os/`.

## 2. What this work changes

### hyalOS, behind the `spike-contract` feature

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/audit/src/replay.rs` | 118-124 | `pre_infer` accepts schema `0` only | changed in Slice 1: under a spike profile, also requires the contract to grant the schema |
| `hyalos-os/audit/src/replay.rs` | 144-153 | `pre_effect` accepts kind `0`, one effect | changed in Slice 1: under a spike profile, also requires the contract to grant the kind |
| `hyalos-os/audit/src/replay.rs` | 378-404 | `execute_with` looks up the profile, frames the task and runs | changed in Slice 1: under a spike profile, reads the contract from input record 0 before running, refusing `BadContract` |
| `hyalos-os/audit/src/record.rs` | 21-52 | the closed `Reason` set | changed in Slice 1: adds `BadContract`, compiled only with the feature |
| `hyalos-os/profile/src/lib.rs` | 35-71 | profiles 1 and 2, `KNOWN` | changed in Slice 1 (and in the slice that needs another spike profile): adds spike profiles to what `lookup` finds, only with its own `spike-contract` feature (ADR-006) |
| `hyalos-os/monitor/src/main.rs` | 182-189 | `read_inputs` splits the inputs file on newlines | kept; Slice 1 adds `spike-run` and `spike-task` (`design.md` §2), which read the contract, SDK and script whole and the data file by lines |
| `hyalos-os/monitor/src/main.rs` | 155-171 | `on_backend` refuses on KVM any profile whose settings are not `CURRENT`'s | kept in Slice 1 (`hyalos.spike/1` has profile 2's settings); changed by the first slice that adds a spike profile with other settings: a spike build compares against the profile its kernel image was built with |
| `hyalos-os/monitor/src/runner.rs` | 65-95 | `REASONS`, the channel's reason bytes | changed in Slice 1: `BadContract` appended at the end, with the feature |
| `hyalos-os/monitor/src/kvm.rs` | 29-41 | RAM 32 MiB, guest slot 1 MiB at `0x70_0000`, heap `0x80_0000..0x180_0000` | changed by the first slice whose guest needs it (over 1 MiB, or more memory): a spike-only layout with more RAM, guest space and heap |
| `hyalos-os/monitor/src/profile_tests.rs` | 152-155 | a source-text check that the kernel image's `PROFILE` line is `PROFILE_2` | kept passing: the first slice that adds a spike kernel image feature leaves the default line verbatim |
| `hyalos-os/kernel-image/src/main.rs` | 32-35, 233-246 | built with `PROFILE_2` and the matching heap | kept in Slice 1; changed by the first slice that adds a spike profile with other settings: a feature `spike-profile-N` selects that profile (and, if needed, its layout) and puts the profile's name after a marker in the image's bytes (`design.md` §2, KVM under a spike profile) |
| `hyalos-os/kernel-image/Cargo.toml` | 9-14 | `wasmi` features; a `selftest` feature | changed by the first slice that adds a spike profile with other settings: its `spike-profile-N` feature; for a float-enabled profile that feature also turns on `wasmi/deterministic` (ADR-007) |
| `hyalos-os/monitor/Cargo.toml` | 24-31 | `test-trust`, `test-switches` | changed in Slice 1: adds `spike-contract`, forwarded to `hyalos-audit` and `hyalos-profile` (ADR-006); changed again by the slice that adds a float-enabled spike profile, whose `spike-contract` turns on `wasmi/deterministic` (ADR-007) |
| `hyalos-os/audit/Cargo.toml` | — | `hyalos-audit`'s features | changed in Slice 1: adds `spike-contract`; changed again by the float-enabled slice, as the monitor's (ADR-007) |
| `.github/workflows/ci.yml` | 233-325 | the `hyalos-os` job: lint, tests on KVM, fuzz, deny | changed in Slice 1: lints and tests the feature too |
| `crates/xtask/src/hyalos.rs` | 18-41 | `UNSAFE_FILES` | kept; a new `unsafe` file under `hyalos-os/` would join it in review |
| `docs/adr/` | — | ADR-0130 to ADR-0135 own hyalOS | Slice 1 adds an ADR for the feature, the contract check and spike profiles; each slice that adds a spike profile or layout adds one (3.4, Slice 4's Files) |

### New, outside the budget

| Path | Fate |
|---|---|
| `spikes/438/` | created: the draft contracts, the SDK generator, the triage script, the MicroPython port, the Monty build, hostile scripts and inputs, measurements and evidence |
| `docs/plans/hyalos-kernel-python/decisions/` | the findings ADR is added in the last slice |

## 3. Shared mechanics to reuse

- The host's call checks run before any guest bytes are read: `pre_infer` then `guest_bytes`, so the
  first failing check is the reason (`hyalos-os/audit/src/replay.rs:236-246`, `hyalos-os/ABI.md:29-32`).
- A refusal ends the run by setting `refused` and returning an error from the host function
  (`hyalos-os/audit/src/replay.rs:176-179`).
- A run is one call: `execute_with(version, profile name, guest, inputs, source, run)`, where `run` is
  the closure that runs the guest on one backend
  (`hyalos-os/audit/src/replay.rs:378`); replay re-runs it and compares every byte of the serialised
  transcript (`hyalos-os/audit/src/replay.rs:407-438`).
- The version 4 task identity frames the profile name, the guest hash and every input with its
  length (`hyalos-os/audit/src/record.rs:205`, `:256-276`), so any list of whole records is bound
  without a new framing.
- `read_transcript` refuses a record whose task is not the one its guest and inputs give
  (`hyalos-os/audit/src/record.rs:150-189`).
- A profile is looked up by exact name, and only if this build's `wasmi` replays it
  (`hyalos-os/profile/src/lib.rs:97-101`); `same_settings` compares everything but name and `wasmi`
  (`hyalos-os/profile/src/lib.rs:87-93`).
- The kernel image configures `wasmi` from the profile in the same order as the process backend
  (`hyalos-os/kernel-image/src/main.rs:233-246`, `hyalos-os/audit/src/replay.rs:284-297`), and checks
  imports and exports the same way (`hyalos-os/kernel-image/src/main.rs:248-262`,
  `hyalos-os/audit/src/replay.rs:307-327`).
- Hostile guests are WAT strings run through `wat::parse_str`, each with the reason it must end with
  (`hyalos-os/monitor/src/main.rs:205-229`); the ABI suite's `on_both` runs one guest on both backends
  and demands the same outcome (`hyalos-os/monitor/src/abi_tests.rs:161-177`).
- The stand-in model is the only nondeterministic part of a run, seeded by the run command
  (`hyalos-os/monitor/src/main.rs:102-123`).
- The issue fixture is `hyalos-os/evidence/fixtures/issues.tsv`, one `<number>\t<title>` per line
  (`hyalos-os/ABI.md:70-71`).
- A test-only behaviour is a Cargo feature that is off by default, and a release-build test proves a
  default build reads none of it (`hyalos-os/monitor/Cargo.toml:26-31`,
  `hyalos-os/monitor/tests/release_switches.rs`).
- The digest guest's committed `hyalos-os/guest/digest.wasm` is what records name; a guest that
  records name is committed as bytes (`hyalos-os/guest/src/lib.rs:177-178`).
- Test names carry the criterion ID they prove, for example `core_s_02_1_e1_…`
  (`hyalos-os/monitor/src/ring_tests.rs:58`). This pack's tests use the prefix `python_`, for example
  `python_s_01_1_e1_…`.

## 4. How verification works here

Run from the repository root. KVM tests skip without `/dev/kvm` unless `HYALOS_REQUIRE_KVM=1` is set
(CI sets it, `.github/workflows/ci.yml:284`); a skip is not a pass, so say which ran.

### Verify

```bash
cargo fmt --manifest-path hyalos-os/Cargo.toml --all --check
(cd hyalos-os/kernel-image && cargo fmt --check)
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml --workspace --all-targets --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches -- -D warnings
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches
HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --test release_switches
cargo xtask lines          # prints "hyalOS: N / 12000 lines" and fails above the budget
cargo xtask hyalos-unsafe  # fails on unsafe outside UNSAFE_FILES or without a SAFETY comment
cargo xtask docs
```

Expected: every command exits 0; the two `attest` builds come first, as in CI
(`.github/workflows/ci.yml:249-252`), since the workspace tests need `forging-notary`; the test runs
end `test result: ok` for every target. Slice 1 adds
the same `clippy` and `test` lines with `task-kernel/spike-contract`, `hyalos-audit/spike-contract`
and `hyalos-profile/spike-contract` added to the features, which must also pass. Each slice's
Verify runs its tests with `HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1`, as CI does, and fails
on any `skipped:` line. The repository-wide checks in `CLAUDE.md` (`cargo fmt --all --check`,
`cargo clippy --workspace …`, `cargo test --workspace --locked`, `cargo xtask layout`,
`cargo deny check`) run as well for any change outside `docs/` and `spikes/`.

A behavioural criterion that depends on building an interpreter (Slices 3 and 4) is proven by a
script under `spikes/438/` that builds the guest from a pinned upstream commit, runs it on both
backends and replays the record; the progress file records its command and output against the ID.

## 5. Glossary

| Term | Meaning |
|---|---|
| backend | One way of running a guest: the process backend (in the monitor's process) or the KVM backend (the kernel image in a machine) |
| contract | In this pack, the spike's draft contract: a small file granting question schemas (`asks`) and effect kinds (`may_touch`). Capsules replaces it |
| `asks` | The contract's list of question schemas a task may ask with `infer` |
| `may_touch` | The contract's list of effect kinds a task may request with `effect` |
| SDK | The Python module generated from a contract, exposing only the calls it grants |
| generator | The program under `spikes/438/` that turns a contract into its SDK |
| triage script | The issue-triage task written in Python against the SDK |
| runtime | A Python interpreter compiled into a guest: MicroPython or Monty |
| runtime guest | A guest whose bytes are an interpreter, with the script delivered as an input record |
| spike profile | An execution profile named `hyalos.spike/N`, known only with the `spike-contract` feature, under which the contract is enforced |
| task identity | The `task` field of a record: `task_v4` over profile, guest hash and every input record |
| hostile guest | A guest or script written to break the rules: an ungranted call, a raw import, an exploit attempt |
| hostile input | A data record written to exploit the runtime rather than the monitor |
| reach | What a hostile input managed to change: labels asked, the effect body, the result or fuel |
| cold start | Time from the run command starting to the guest's first host call |
| fuel | `wasmi`'s instruction budget, recorded in every record as `fuel_used` |
| `BadContract` | The refusal for a spike run whose contract is missing, unreadable, empty or not a contract, before the guest loads |
