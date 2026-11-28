# Context — what exists today

Every citation below was checked on 2026-11-25 at commit `bf09657`. Preflight (README rule 2) means
re-checking the ones your slice touches. Paths are from the repository root. This file describes
what exists; it chooses nothing the PRD, the ADRs or the design will decide.

## 1. Repository shape

- **hyalOS is its own Cargo workspace** in `hyalos-os/` (ADR-0130). Members are `monitor` (package
  `task-kernel`), `attest`, `attest-check`, `profile` (package `hyalos-profile`), `audit` (package
  `hyalos-audit`) and `verify`. `kernel-image` (package `hyalos-kernel`) and `guest` are excluded and
  keep their own manifests and lockfiles, because the kernel image needs `panic = "abort"` and the
  guest builds for WebAssembly (`hyalos-os/Cargo.toml:1-6`, `hyalos-os/monitor/Cargo.toml:2`,
  `hyalos-os/audit/Cargo.toml:2`, `hyalos-os/kernel-image/Cargo.toml:2`). The toolchain is pinned in
  `rust-toolchain.toml:2`. Nothing in `core/` depends on hyalOS (`CLAUDE.md:43-49`).
- **Two backends run one guest.** The process backend runs the guest under `wasmi` in the monitor's own
  process (`hyalos-os/audit/src/replay.rs:343-418`). The KVM backend boots the kernel image, which runs
  the guest under its own `wasmi` (`hyalos-os/kernel-image/src/main.rs:260-349`), inside a machine owned
  by a confined machine runner process (`hyalos-os/monitor/src/runner.rs:1-15`, ADR-0132). The monitor
  chooses KVM when `HYALOS_KERNEL` names a kernel image (`hyalos-os/monitor/src/main.rs:699-713`).
- **Every host call is answered by one `Host`.** `hyalos_audit::Host` holds the inputs, answers and
  effects and owns the call checks (`hyalos-os/audit/src/replay.rs:94-210`). The process backend calls it
  from its linker (`hyalos-os/audit/src/replay.rs:249-324`). The KVM holder calls the same methods
  through the `Calls` trait (`hyalos-os/monitor/src/kvm.rs:328-355`) when the runner relays a request
  (`hyalos-os/monitor/src/runner.rs:404-433`). A check added to `Host` therefore holds on both backends.
- **The guest interface** is `hyalos.abi/1` (`hyalos-os/ABI.md:15-71`): four imports from module `hyalos`
  (`read`, `infer`, `effect`, `return`), a `run` export and a `memory` export, every refusal named. The
  document says a section is never edited once a backend implements it, and the conformance suite pins
  each section's digest (`hyalos-os/ABI.md:9-13`, `hyalos-os/monitor/src/abi_tests.rs:12-20`). A record
  does not carry the interface name, and the core pack's ADR-004 says a second version's pack must add
  it to the record (`hyalos-os/ABI.md:5-7`,
  `docs/plans/hyalos-kernel-core/decisions/ADR-004-name-the-guest-interface-outside-the-record.md`,
  Consequences).
- **Execution profiles** fix the interpreter settings and limits a run and its replay depend on; a changed
  profile is a new name and the old ones stay (`hyalos-os/profile/src/lib.rs:1-6`, `:38-69`, ADR-0134,
  parent ADR-002 at `docs/plans/hyalos-kernel/decisions/ADR-002-keep-wasmi-and-name-the-execution-profile.md`).
  New runs use `hyalos.profile/2` (`hyalos-os/profile/src/lib.rs:69`).
- **The record** is a `Transcript` (`hyalos-os/audit/src/record.rs:107-127`), version 4 for new runs
  (`hyalos-os/audit/src/record.rs:17`), naming its profile and a framed task identity
  (`hyalos-os/RECORD.md:14-20`, `:39-43`).
- **The notary** is a separate program from the monitor. It numbers attestations per run, spends a
  ticket from a customer authorisation at a run's first session, and admits one session at a time
  (`hyalos-os/attest/src/bin/notary.rs:230-296`, `:1320`, `:1328-1332`, `hyalos-os/RECORD.md:56-58`).
- **The Python guest spike** (#438) left a draft contract, a generator, MicroPython and Monty guests,
  hostile inputs, measurements and committed evidence under `spikes/438/`, with the monitor's side
  behind a `spike-contract` feature (`spikes/438/README.md:1-28`, ADR-0136, ADR-0137). Its findings
  ADR names MicroPython as the runtime to adopt and says what this pack decides
  (`docs/plans/hyalos-kernel-python/decisions/ADR-008-findings-adopt-micropython-monty-is-not-ready.md:127-150`).
- **Budget and `unsafe`.** hyalOS's non-test Rust may not pass 12,000 lines
  (`crates/xtask/src/hyalos.rs:14`); the count covers only `.rs` files outside `tests/` and
  `*_tests.rs` (`crates/xtask/src/hyalos.rs:46-52`), so Python, C and shell are not counted. The last
  recorded count is 9,347 (`docs/plans/hyalos-kernel-python/progress/slice-05.md:70`). `unsafe` is
  allowed only in the files on `UNSAFE_FILES` (`crates/xtask/src/hyalos.rs:18-39`, ADR-0133).
- **No second job and no later pack exist.** The one job is the issue digest
  (`hyalos-os/guest/src/lib.rs:1-4`, `hyalos-os/monitor/src/live.rs:260-262`). The connectors and engine
  packs are briefs only (`docs/plans/hyalos-kernel/pack-briefs.md:117-131`), and `docs/plans/` holds no
  folder for either.
- **The brief's stale citation.** `docs/plans/hyalos-kernel/pack-briefs.md:99` cites the newline split at
  `kernel/src/main.rs:441`. The repository has no `kernel/` directory. The split is
  `hyalos-os/monitor/src/main.rs:202`, inside `read_inputs` (`:199-206`).
- **`docs/adr/`** ends at ADR-0137 (`docs/adr/0137-hyalos-spike-2-floats-and-a-kernel-image-that-names-its-profile.md`).
  The repo rule: never edit an Accepted ADR's Context, Decision or Consequences; write a new one
  (`CLAUDE.md:9-12`).

## 2. What this work changes

Fate says what this pack does to a row. Slice numbers are not given here:
`plan.md` maps each row's change to its slice. The index below is the Brief's list of areas and the table that
holds each.

| Brief area | Table |
|---|---|
| #503 fix and a committed MicroPython record on both backends | A |
| Length-framed inputs, `hyalos.abi/2`, a new record version, task identity, contract, SDK and script as inputs under the real profile | B |
| The monitor's contract check replaces the spike's | C |
| Closed answer types: `bool`, a bounded integer, a choice from a declared list | D |
| Per-task question limit, its default, and the #411 measurements | E |
| The SDK generator, kept outside the monitor | F |
| The audit report's statement of Python ADR-008's limit | G |
| Line budget, CI, ADRs | H |
| A second job (the field-list check), the adversarial read | §1 (no code exists for either) |

### A. The kernel image and #503

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/kernel-image/Cargo.toml` | 11 | `wasmi` 2.0.3 with default features off and `stable`, `validate`, `memory64`, `auto-dispatch`, `libm` | changed by the #503 fix, if the fix is a dispatch feature |
| `hyalos-os/monitor/src/kvm.rs` | 29-41, 278 | RAM 32 MiB; the kernel's stack is `0x1e0_0000..RAM` (2 MiB) with `rsp` at `RAM - 8`; heap `0x80_0000..0x180_0000`; guest slot 1 MiB | kept, or changed if the fix grows the stack |
| `spikes/438/README.md` | 99 | the `process-only: micropython` line: the default image runs out of its 2 MiB stack at about 10,000 guest loop iterations and ends `KernelFault`; `spike_evidence_tests.rs` reads this line | changed: removed once MicroPython runs on KVM |
| `spikes/438/micropython/run.sh` | 8-15, 84-90 | runs the triage case once on KVM and fails unless it ends `KernelFault`, so a #503 fix is noticed | changed: that check is inverted by the fix |
| `spikes/438/evidence/expected.tsv` | 2-5, 11-18 (column `backends`) | every MicroPython case, the eight hostile cases included, is listed `process` (12 in all) | changed: cases listed `both`, with a committed record |
| `hyalos-os/monitor/src/spike_evidence_tests.rs` | 168-233 | `python_committed_evidence_replays_on_both_backends` runs each case on the process backend, and on KVM only for a case listed `both`; it compares the written transcript to the committed record and replays it | kept; its KVM half starts to run for MicroPython |
| `spikes/438/guests/micropython.wasm` | | the committed guest, 249,874 bytes, SHA-256 `590822a7…3b40` (`spikes/438/README.md:94-95`); built by `spikes/438/micropython/build.sh` in a pinned container | kept unchanged: the plan needs no rebuild, and a rebuild is a stop for the developer (Slice 8) |
| `spikes/438/micropython/port/main.c` | 28-37 | a fixed 2 MiB heap that is never collected | kept |

### B. Input form, interface version, record version, task identity

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/monitor/src/main.rs` | 199-206 | `read_inputs` splits the file on `\n` and drops empty lines. Callers: `run` `:726`, `live` `:761`, `fuzz` `:911`, `task` `:929`, `attacks` `:945`, `bench` `:953`, and `spike.rs:84` | changed: framed inputs |
| `hyalos-os/ABI.md` | 15-70 | the `hyalos.abi/1` section; line 70 says each input record is one line of UTF-8 with no newline | kept (never edited); a new `hyalos.abi/2` section is added |
| `hyalos-os/monitor/src/abi_tests.rs` | 12-20, 393-410 | `PINNED` holds one digest; the test fails if the `abi/1` section changes | changed: a second pinned digest |
| `hyalos-os/audit/src/record.rs` | 7-8, 17, 154-193, 236-248 | `ABI` is `hyalos.abi/1`; `CURRENT_VERSION` is 4; `read_transcript` accepts versions 1 to 4; `profile_for` maps a version and name to a profile | changed: a new record version |
| `hyalos-os/kernel-image/src/main.rs` | 28-31 | the kernel image's own copy of the `ABI` name, held equal by a test | changed |
| `hyalos-os/RECORD.md` | 3, 14-20, 39-43 | covers versions 1 to 4; a record cannot change version | changed |
| `hyalos-os/audit/src/record.rs` | 203-216, 260-280 | the version 4 task identity already frames the profile name, the guest hash, the input count and each input's length and bytes | kept |
| `hyalos-os/monitor/src/spike.rs` | 66-87 | under a spike profile, records 0 to 2 are the contract, SDK and script as whole files, and the data file's lines follow | unchanged until Slice 12, then deleted with the spike rig; the real profile's framed inputs are new code, not a change to it |
| `hyalos-os/guest/src/lib.rs` | 26-32 | the digest guest reads at most 200 records, keeps 16,384 bytes per record, 4,096 per title | kept; the digest contract's `accepts` states the same bounds (design.md §2.2, the digest contract row) |
| `hyalos-os/monitor/src/kvm.rs` | 323 | `RECORD_MAX` is the heap's size, 16 MiB: the largest record the KVM backend can write into the guest | kept |

### C. The contract check

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/audit/src/contract.rs` | 1-63 | the draft contract: canonical bytes `{"asks":[…],"may_touch":[…]}`, at most 4,096 bytes, only schema `0` and effect `0`; compiled only with `spike-contract` | replaced by the real contract |
| `hyalos-os/audit/src/replay.rs` | 99-102, 139-156, 176-196, 437-446 | under a spike profile `pre_infer` refuses a schema `asks` lacks (`UnknownSchema`), `pre_effect` a kind `may_touch` lacks (`UnknownEffect`), and `execute_with` reads input record 0 as the contract before the guest loads (`BadContract`) | changed |
| `hyalos-os/profile/src/lib.rs` | 71-106, 130-135 | `hyalos.spike/1` and `hyalos.spike/2` exist only with `spike-contract`; `enforces_contract` is true for any name starting `hyalos.spike/`; no other profile enforces a contract | changed |
| `hyalos-os/audit/src/record.rs` | 21-56 | the closed `Reason` set; `BadContract` is feature-gated | changed |
| `hyalos-os/monitor/src/runner.rs` | 64-101 | `REASONS` is the channel's reason bytes (29, plus `BadContract` last with the feature); the comment says a new reason goes at the end | changed |
| `hyalos-os/monitor/src/spike.rs` | 1-178 | `spike-run`, `spike-task`, the kernel-image profile marker | deleted in Slice 12 with the spike rig (ADR-011) |
| `hyalos-os/monitor/src/main.rs` | 145, 156-188, 936-940 | `named` profile is `CURRENT`'s; `on_backend` refuses on KVM a profile whose settings differ from the image's; the spike commands | changed |
| `hyalos-os/monitor/Cargo.toml`, `audit/Cargo.toml`, `profile/Cargo.toml`, `kernel-image/Cargo.toml` | `:37`, `:18`, `:14`, `:18` | the `spike-contract` and `spike-profile-2` features | changed |
| `.github/workflows/ci.yml` | 70, 241-337, 339-372 | the change filter matches `hyalos-os/` and `spikes/438/`; jobs `hyalos-os` and `hyalos-os-spike` | changed if the feature goes |
| `docs/adr/0136-hyalos-spike-contract-check.md`, `0137-…` | | own the spike contract and `hyalos.spike/2` | `Superseded by [ADR-0138](0138-hyalos-abi-2-and-record-version-5.md)` (the new ADR for `hyalos.abi/2` and record version 5), in Slice 12's last commit (ADR-038) |

### D. Closed answer types

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/ABI.md` | 25, 40, 44 | `infer` takes a schema and returns a label; schema `0` returns `0` to `3`; `UnknownSchema` is "schema is not `0`"; `ModelRefused` is "answer is not a label" | kept; `abi/2` states the new rules |
| `hyalos-os/audit/src/record.rs` | 9-15, 58-72 | `MAX_QUESTION` 4,096; `LABELS` is 4; `Answer` holds `schema`, `question_sha256`, `label: i32`, `model`, `response_id`, `attestation`, and `provider_proofs`, left out when empty (`:70-71`) | changed |
| `hyalos-os/audit/src/replay.rs` | 45-67, 139-174 | a replay accepts an answer only if its schema and question hash match and `label` is in `0..LABELS`; `pre_infer` accepts schema `0` only | changed |
| `hyalos-os/monitor/src/main.rs` | 50-78, 102-125 | the `Source` for each mode; the stand-in model picks a label from keywords and a seed, modulo `LABELS` | changed |
| `hyalos-os/monitor/src/live.rs` | 19, 210-251 | the question must be UTF-8; the tool is run with the position; the answer's `label` must be an integer in `0..4` and its presentation hash must match the kept file | changed |
| `hyalos-os/attest/src/lib.rs` | 22-25, 146-165, 195-212, 255-294 | one approved request template, with a `json_schema` whose `label` is an enum of the four words; `approved` accepts only that template; `answer` maps a word to its index | changed |
| `hyalos-os/attest/src/main.rs` | 499-502, 458, 562, 209-212 | the tool reads one question from standard input (at most `MAX_QUESTION`) and prints `label`; it refuses to attest a request over `MAX_SENT` − 256 = 7,936 bytes (`MAX_SENT` is 8,192, `hyalos-os/attest/src/lib.rs:26`), "the question is too large to attest", so a question whose JSON escaping pushes the request past that is refused by the tool, though a plain 4,096-byte title fits | changed (the bound kept) |
| `hyalos-os/audit/src/answers.rs` | 118-126 | the audit holds the notary's seen `label` equal to the record's | changed |
| `hyalos-os/attest-check/src/lib.rs` | 20, 218-228, 316-338 | the verifier's own check of a stored presentation, a dependency of `audit` (`hyalos-os/audit/Cargo.toml:22`) and a dev-dependency of `verify` (`hyalos-os/verify/Cargo.toml:16-17`): its own `LABELS`, the approved request `body` with the label enum, and `answer`, which maps the seen label to its index | changed |
| `hyalos-os/attest/src/bin/canary.rs` | 44 | the canary's literal copy of the approved request, with the four-label enum | changed |
| `hyalos-os/kernel-image/src/main.rs` | 199-214 | the `infer` import passes `schema`, the question pointer and length to the monitor | changed |

The notary side is pinned to the tool: `attest/src/lib.rs:22` says its values "must match the monitor's
`live.rs`". `infer` returns one `i32` today (`hyalos-os/ABI.md:25`), so a model never returns text to a
guest.

### E. The question limit and #411

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/audit/src/replay.rs` | 139-174 | `pre_infer` and `infer` check no count of questions; an answer's position is `self.answers.len()` | changed: a per-run limit |
| `hyalos-os/monitor/src/live.rs` | 69-80, 210-232 | `asked` counts calls; the first call waits for the intent link's signed time; a call is never retried | kept |
| `hyalos-os/attest/src/bin/notary.rs` | 230-296 | `spend` creates the run's entry and ticket at its first session; each session adds 1 to a `u64` count (`:100-103`); no cap on sessions per run | kept |
| `hyalos-os/attest/src/bin/notary.rs` | 1320, 1328-1332 | `Semaphore::new(1)`; a connection that arrives while a session is open is closed and logged `refused busy` | kept |
| `hyalos-os/attest/src/bin/notary.rs` | 1175, 1209 | `notary report` prints the cost figures, including "link bytes per attestation" (`:1209`) | kept |
| `hyalos-os/audit/src/answers.rs` | 226-228 | an attempt's budget must be 1 to 100 and its ticket inside it | kept |
| `hyalos-os/monitor/src/main.rs` | 708, 784 | the KVM deadline is 30 s by default; a live run's is `max(600, inputs × 35 + 60)` seconds, from the inputs' count | changed if the limit moves it |
| `hyalos-os/attest/tests/cost.rs` | 118-170 | 40 sessions against the stand-in provider; no test contacts the real one | kept |

What has been measured against the real provider is one call: 7.42 s over two machines and an SSH
tunnel, 304,596 KB peak on the operator's side
(`docs/plans/hyalos-kernel-notary/progress/slice-03.md:36`, `:52`, `:85`). The parent README gives
1.9 s and about 650 MB peak per call (`docs/plans/hyalos-kernel/README.md:95`), measured with both
sides in one process (`docs/plans/hyalos-kernel-notary/progress/slice-03.md:85`). At 1.9 s, 200 calls
take about 6 minutes; at the two-machine 7.42 s, about 25 minutes. The stand-in gave 0.595 s per call over 40 calls
(`docs/plans/hyalos-kernel-notary/progress/slice-06.md:44`). The triage script asks one question per
issue, up to 200 (`spikes/438/scripts/triage.py:7`, `:16-23`). Nothing in the repository measures a run
of 200 attested calls, or any call on the largest input a task may accept.

### F. The SDK generator

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `spikes/438/generate/generate.py` | 1-80 (197 lines) | a pure function from the draft contract's canonical bytes to one Python source file; a fixed name table (`ask.classify` for schema `0`, `notify.team` for effect `0`); it re-implements the Rust parser | changed: generates from the real contract |
| `spikes/438/generate/check.sh` | | generates twice and compares with `spikes/438/sdk/`; no workflow runs it (§4) | changed |
| `spikes/438/sdk/*.py`, `spikes/438/contracts/*` | | committed output and the three draft contracts | replaced |
| `spikes/438/scripts/triage.py` | 1-51 | the triage task against the SDK | kept, or adapted |

### G. The audit report

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `hyalos-os/audit/src/report.rs` | 1-103 (of 127) | `audit_report` states what a pass proved and did not; its wording is "a public contract, pinned by the golden reports". It takes no input that says a guest is a runtime guest; the record names only a profile and a guest hash | changed: states Python ADR-008's limit |
| `hyalos-os/audit/tests/golden/*.report` | | the pinned reports, checked by `hyalos-os/verify/tests/cli.rs:160-187` and `hyalos-os/monitor/tests/audit_cli.rs:268-272` | kept, or re-pinned |
| `hyalos-os/audit/src/audit.rs` | 66-160 | the whole audit: read the record, check the guest, the chain, the answers, replay, then the report | changed if the report needs more inputs |

Python ADR-008's wording, which the report must carry, is at
`docs/plans/hyalos-kernel-python/decisions/ADR-008-findings-adopt-micropython-monty-is-not-ready.md:99-125`;
the statement itself is `:115-118`.

### H. Budget, CI and documents

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `crates/xtask/src/hyalos.rs` | 14, 18-39 | the 12,000-line budget; the eleven `UNSAFE_FILES` | kept; raising the budget takes a new ADR (`docs/adr/0133-hyalos-has-a-line-budget-and-a-list-of-unsafe-files.md`, Consequences) |
| `hyalos-os/review/scope.toml` | 20-40 | one row per component and part, held to the lockfiles by `cargo xtask review-scope` (ADR-0135); the `wasmi` row's reason (`:29`) names `auto-dispatch`, the feature #503 concerns | changed if a dependency or feature changes |
| `docs/adr/` | | ADR-0130 to ADR-0137 own hyalOS | changed: new ADRs, with `docs/adr/README.md`'s rows |
| `hyalos-os/ABI.md`, `hyalos-os/RECORD.md` | | the two documents the interface and the record are held to | changed |

## 3. Shared mechanics to reuse

- **The call checks run before any guest bytes are read**: `pre_infer` or `pre_effect`, then
  `guest_bytes`, so the first failing check is the reason (`hyalos-os/audit/src/replay.rs:280-285`,
  `:298-303`, `hyalos-os/ABI.md:30-32`).
- **A refusal ends the run** by setting `refused` and returning an error from the host function
  (`hyalos-os/audit/src/replay.rs:219-222`).
- **A run is one call to `execute_with(version, profile name, guest, inputs, source, run)`**, where `run` is
  the closure for one backend (`hyalos-os/audit/src/replay.rs:421-459`). Replay re-runs it and compares
  the serialised transcript byte for byte (`hyalos-os/audit/src/replay.rs:462-493`).
- **The task identity** frames every input with its length (`hyalos-os/audit/src/record.rs:260-280`).
  `read_transcript` refuses a record whose task is not the one its guest and inputs give
  (`hyalos-os/audit/src/record.rs:176-190`).
- **A profile is looked up by exact name**, and only if this build's `wasmi` replays it; `same_settings`
  compares everything but name and `wasmi` (`hyalos-os/profile/src/lib.rs:113-128`, `:139-143`).
- **Both backends map a profile to the interpreter the same way**, and a test compares the two sources
  (`hyalos-os/audit/src/replay.rs:326-340`, `hyalos-os/kernel-image/src/main.rs:244-258`,
  `hyalos-os/monitor/src/profile_tests.rs:147-170`).
- **A new reason** is added to the `Reason` enum, appended to `REASONS`, and named in `ABI.md`; the
  exhaustive `match` in `every_reason` fails to compile until the test lists it
  (`hyalos-os/audit/src/record.rs:21-56`, `hyalos-os/monitor/src/runner.rs:66-68`,
  `hyalos-os/monitor/src/abi_tests.rs:102-148`).
- **A change to the interface** needs a new name, a new `ABI.md` section, a new pinned digest and an ADR
  (`hyalos-os/monitor/src/abi_tests.rs:20`).
- **Every `Calls` method has one `Host` implementation**, so a check in `Host` covers the process backend
  and the KVM holder (`hyalos-os/monitor/src/kvm.rs:328-355`).
- **Hostile guests are WAT strings** run through `wat::parse_str`, each with the reason it must end with
  (`hyalos-os/monitor/src/main.rs:208-244`). `on_both` runs one guest on both backends and demands the
  same outcome (`hyalos-os/monitor/src/abi_tests.rs:162-177`).
- **The KVM gate**: a test runs on KVM when `/dev/kvm` opens, fails when `HYALOS_REQUIRE_KVM` is set and it
  does not, and otherwise prints `skipped: <test> needs KVM`. KVM tests build the kernel image from the
  tree each time (`hyalos-os/monitor/src/profile_tests.rs:55-61`, `:66-95`, `:99-136`).
- **The committed-evidence harness** reads `spikes/438/evidence/expected.tsv`, one case per line with its
  guest, contract, SDK, script, data, seed, backends, outcome and record
  (`hyalos-os/monitor/src/spike_evidence_tests.rs:26-61`, `:63-106`).
- **Golden reports** pin the audit's wording (`hyalos-os/verify/tests/cli.rs:160-187`).
- **The stand-in model** is the only nondeterministic part of a run, seeded by the run command
  (`hyalos-os/monitor/src/main.rs:102-125`).
- **The issue fixture** is `hyalos-os/evidence/fixtures/issues.tsv`, 40 lines of `<number>\t<title>`
  (`wc -l`); `hyalos-os/ABI.md:70` gives the one-line record form it follows.
- **A test-only behaviour is a Cargo feature that is off by default**, and a release-build test proves a
  default build reads none of it (`hyalos-os/monitor/Cargo.toml:24-37`,
  `hyalos-os/monitor/tests/release_switches.rs:1-12`).
- **Test names carry the criterion ID**, for example `core_s_01_2_e1_the_unchanged_interface_passes`
  (`hyalos-os/monitor/src/abi_tests.rs:393`); the Python pack used `python_s_01_1_e1_…`
  (`hyalos-os/monitor/src/spike_tests.rs:174`).
- **The attested question's one template and its enum** are in `hyalos-os/attest/src/lib.rs:146-165`; the
  monitor's copy of the labels is `hyalos-os/monitor/src/live.rs:19`; the record's is
  `hyalos-os/audit/src/record.rs:14`; the verifier's is `hyalos-os/attest-check/src/lib.rs:20`, with its own
  request `body` at `:218`; and the canary holds the approved request literally at
  `hyalos-os/attest/src/bin/canary.rs:44`; and the canary's probe holds the approved body again as
  `APPROVED` at `hyalos-os/attest/probes/hidden_header.py:24`. There are six copies, and `attest/src/lib.rs:22`
  says they must match. ADR-004 counts five (it leaves out the probe, which changes with the canary); its
  count stands.
- **ADR rules**: an Accepted ADR is superseded by a new one, and a feature's ADR goes in as Proposed
  (`CLAUDE.md:9-12`).

## 4. How verification works here

Run from the repository root. KVM tests skip without `/dev/kvm` unless `HYALOS_REQUIRE_KVM=1` is set (CI
sets it, `.github/workflows/ci.yml:290-299`); a skip is not a pass, so say which ran.

```bash
cargo fmt --manifest-path hyalos-os/Cargo.toml --all --check
(cd hyalos-os/kernel-image && cargo fmt --check)
(cd hyalos-os/guest && cargo fmt --check)
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml --workspace --all-targets --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches -- -D warnings
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml --workspace --all-targets -- -D warnings
(cd hyalos-os/kernel-image && cargo clippy --release --locked -- -D warnings && cargo clippy --release --locked --features selftest -- -D warnings)
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches
HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --test release_switches
cargo xtask lines
cargo xtask hyalos-unsafe
cargo xtask docs
cargo xtask layout
cargo xtask review-scope
cargo xtask review-package
```

These are the core of CI's `hyalos-os` job (`.github/workflows/ci.yml:241-338`), with the two `attest` builds
first since the workspace tests need `forging-notary` (`.github/workflows/ci.yml:259-260`). The job also runs
the `forging-notary` clippy (`:282`), the guest's clippy, a fuzz step, `probe-host` and `cargo deny`. The
`cargo xtask` checks run in the `lint` job (`.github/workflows/ci.yml:106-123`), not `hyalos-os`. Expected: every
command exits 0 and every test run ends `test result: ok`. `cargo xtask lines` prints
`hyalOS: N / 12000 lines (P%)` (`crates/xtask/src/hyalos.rs:246-250`) and fails above the budget through
`within_budget` (`:259`, defined at `:121`); after every slice N is at most 12000 (each slice's Stop conditions).
`cargo xtask layout` prints `layout ok: N workspace crates` (`crates/xtask/src/main.rs:227`), `cargo xtask review-scope` prints `hyalOS review scope: N rows agree with M lockfiles` (`crates/xtask/src/scope.rs:496-500`), and `cargo xtask review-package` prints `review package ok: N sections, commit under review is an ancestor of HEAD, M code references` (`crates/xtask/src/package.rs:256`). No slice's Verify repeats these `cargo xtask` checks; each slice's Verify ends by running this section. Slice 1's Verify alone also runs `cargo xtask review-scope` itself, as the proof of its criterion 1.1.
`cargo xtask hyalos-unsafe` prints `hyalOS: unsafe only in the N listed files, each use with a SAFETY
comment` (`crates/xtask/src/hyalos.rs:267`). The repository-wide checks in `CLAUDE.md:28-45` also run
for a change outside `docs/` and `spikes/`.

While the `spike-contract` feature exists, CI also lints and tests every hyalOS crate with it
(`.github/workflows/ci.yml:339-372`); add `task-kernel/spike-contract`, `hyalos-audit/spike-contract` and
`hyalos-profile/spike-contract` to the feature lists.

Scripts under `spikes/438/` are run by hand. No workflow runs them: `git grep -n "spikes/" -- .github/workflows`
finds only `ci.yml`'s change filters and the comment above them (`.github/workflows/ci.yml:20`, `:49`, `:68-70`), which decide whether a job
runs and run no script.
`spikes/438/generate/check.sh` ends `check: ok`; `spikes/438/micropython/build.sh` builds twice and
compares, needing docker; `spikes/438/micropython/run.sh` ends `micropython: ok` (`run.sh:94`), which it
also prints on a host without KVM, after its no-KVM line (`run.sh:92`), so only its KVM-leg lines prove
the KVM check ran (`spikes/438/README.md:101-104`). A criterion with no test is proven by a script recorded with its
command and output against the criterion ID in the slice's progress file, as the Python pack did.

## 5. Glossary

| Term | Meaning |
|---|---|
| backend | One way of running a guest: the process backend (in the monitor's process) or the KVM backend (the kernel image in a machine) |
| monitor | The `task-kernel` program in `hyalos-os/monitor`: it runs a guest, answers its four calls and writes the record |
| kernel image | The program in `hyalos-os/kernel-image` that boots on KVM and runs the guest under `wasmi` |
| machine runner | The confined process that owns a KVM machine (`hyalos-os/monitor/src/runner.rs`) |
| guest | A WebAssembly module that imports the four calls and exports `run` and `memory` |
| runtime guest | A guest whose bytes are an interpreter (here MicroPython), with the script delivered as an input |
| task | A guest with its inputs; the unit a customer authorises |
| task identity | The `task` field of a record: a hash over the profile name, the guest hash and every input, each framed by its length (`task_v4`) |
| input record | One entry of the inputs list, read by a guest with `read(index, ptr, cap)` |
| framed input | In this pack, an input file form in which each record's length is stated, so a record may hold any bytes, newlines included |
| `hyalos.abi/N` | A version of the guest interface; a record does not name it today |
| execution profile | A named set of interpreter settings and limits (`hyalos.profile/2` for new runs); a changed profile is a new name |
| record | The transcript of a run: guest hash, inputs, answers, effects, result, fuel; version 4 today |
| contract | In this pack, the statement of what a task may ask, touch and return, its limits and its question limit, bound by the customer's authorisation of the task identity that covers it |
| draft contract | The spike's contract: canonical JSON with `asks` and `may_touch` only |
| `asks`, `may_touch`, `accepts`, `returns`, `limits` | The draft shape's fields in `docs/plans/hyalos-kernel/pack-briefs.md:104-115`: question schemas, effect kinds, input types and limits, result type, and fuel, memory and sizes |
| SDK | The Python module generated from a contract, exposing only the calls it grants |
| generator | The program that turns a contract into its SDK |
| script | The task's Python source, delivered as an input |
| question | One `infer` call |
| answer | What the approved model returned for a question, with its attestation |
| closed answer type | An answer type with a fixed set of values: `bool`, a bounded integer, or a choice from a declared list |
| question limit | The most questions a run may ask, declared in the contract and enforced by the monitor |
| effect | A call to `effect`; today one notification, one per run |
| notary | The customer's separate attestation program (`hyalos-os/attest/src/bin/notary.rs`) |
| attestation, presentation | The notary's signed proof of one provider session, kept beside the record |
| authorisation | The customer's decision that one exact task may have 1 to 100 attempts (`hyalos-os/RECORD.md:56-58`) |
| ticket | One attempt spent from an authorisation at a run's first session |
| fuel | `wasmi`'s instruction budget, recorded in every record as `fuel_used` |
| hostile input | A data record written to exploit the runtime rather than the monitor |
| `BadContract` | The spike's refusal for a run whose input record 0 is not a contract |
| #503 | The kernel image's stack overflow on long guest loops; building the image with `portable-dispatch` and nothing else changed removes it (`docs/plans/hyalos-kernel-python/progress/slice-03.md:126`) |
| #411 | The issue for the inference limit and the real attestation cost (`docs/plans/hyalos-kernel/coverage.md:52`) |
