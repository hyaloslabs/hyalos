# Slice 3 — The triage task runs under MicroPython

| | |
|---|---|
| **Status** | done |
| **Issue** | #496 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-25 |

## Preflight

Checked on the branch's base, `c97212e` (Slice 2 merged).

| Citation | Holds |
|---|---|
| `hyalos-os/guest/src/lib.rs:14-24` the digest guest's four imports; `:107-108` its `run` export | holds |
| `hyalos-os/ABI.md:15-20` `hyalos.abi/1`: `run`, `memory`, no start function, the four imports | holds |
| `hyalos-os/audit/src/replay.rs:300-327` what the loader accepts | differs, not a contract: those lines are now the linker's last two calls and `engine`; the import and export checks are at `:349-369`, and say what the plan says |
| `hyalos-os/audit/src/replay.rs:407-438` replay from a record | differs, not a contract: those lines end `in_process`; replay is `replay_with`, which `task-kernel`'s `replay` (`hyalos-os/monitor/src/main.rs:175`) calls, as the test does |
| `hyalos-os/monitor/src/abi_tests.rs:161-177` both-backend comparison | holds |
| `hyalos-os/monitor/src/kvm.rs:33` `GUEST_MAX`, `:29-41` the memory guards | holds |
| `hyalos-os/monitor/src/profile_tests.rs:114-121` the KVM gate that fails on a skip under `HYALOS_REQUIRE_KVM` | holds |
| `hyalos-os/monitor/src/main.rs:165` `same_settings` | holds |
| `.github/workflows/ci.yml:284-292` the workspace test with `HYALOS_REQUIRE_KVM` | holds |
| `design.md` §2 Python surface, Runtime guest `run` | holds |
| `spikes/438/kernel.sh`, `contracts/`, `sdk/`, `scripts/triage.py` as Slice 2 left them | holds |

## Acceptance

| ID | How it was shown |
|---|---|
| S-04.1 | Met as a recorded result: the process backend gives the transcripts, and every record replays there. On KVM the default kernel image cannot run MicroPython (Deviations, `process-only: micropython` in `spikes/438/README.md`, issue #503) |
| S-04.1.e1 | `expected.tsv` case `micropython-triage-0`, listed `process`: `python_committed_evidence_replays_on_both_backends` re-runs it, compares the transcript with `evidence/micropython-triage-0.json` byte for byte, and replays the record. `run.sh` checks the KVM leg still ends `KernelFault` |
| S-04.1.e2 | Case `micropython-triage-7`, the same way. Its notification differs from seed 0's (`python_3_3_…` asserts it) |
| S-04.2 | S-04.2.e1 |
| S-04.2.e1 | Case `micropython-no-notify` (the `no-notify` contract and SDK, the triage script): `result error AttributeError effects 0`, with the 40 `ask.classify` answers recorded before it (`ok: 43 inputs, 40 inferences, 0 effect`) |
| S-04.3 | S-04.3.e1 |
| S-04.3.e1 | Case `micropython-raw-effect` (`scripts/raw-effect.py` under `no-notify`): `refused UnknownEffect`, no record written |
| S-04.4 | The two rows below |
| S-04.4.e1 | `python_s_04_4_e1_the_micropython_guest_imports_only_the_four_calls`: `wasmi`'s module reader lists exactly `hyalos.read`, `infer`, `effect` and `return`, all functions, and the exports include `run` and `memory` |
| S-04.4.e2 | `python_s_04_4_e2_a_micropython_guest_with_one_more_import_is_refused`: a copy with one more import (`env.leak`, a mutable `i32` global, so the copy is still a valid module) is refused `BadImport` on both backends; the import check comes before the guest runs |
| 3.1 | `spikes/438/micropython/build.sh`: fetches tag `v1.29.0` from `https://github.com/micropython/micropython`, stops unless it is `0fd6c573ea815774668bbb16b8e197c8822368b2`, builds twice from a clean tree at one path with `SOURCE_DATE_EPOCH=0`, compares, and writes `guests/micropython.wasm`: `micropython.wasm: 249874 bytes, sha256 590822a76c8ccbb2bd74d920bea3134ba671ad96c594f8888657fbd3b7c73b40`, which `README.md` records |
| 3.2 | `python_committed_evidence_replays_on_both_backends` in `hyalos-os/monitor/src/spike_evidence_tests.rs`, with the `spike-contract` feature. It replays every MicroPython record on the process backend. Its KVM leg runs each `both` case behind the existing gate; MicroPython's cases are `process`, so the KVM half of 3.2 is the recorded result above. It fails, naming the case, when the `process-only:` line is missing, a record differs, or a `*.json` is no case's record (each tried once, then restored) |
| 3.3 | `python_3_3_micropython_triage_matches_the_digest_guest`: for seeds 0 and 7, the MicroPython run's notification and result are byte-identical to the digest guest's under `hyalos.profile/2` on the fixture's lines alone |
| 3.4 | The guest runs under `hyalos.spike/1`: 249,874 bytes (under `GUEST_MAX`, 1 MiB), 4 MiB of memory (the profile's limit), 33,127,745 fuel at seed 0 (under 200,000,000). `README.md`: `micropython profile: hyalos.spike/1`. No new profile, kernel image feature or layout |

### Verify, as run

From the repository root, on a host with KVM and Landlock. Every line exited as expected:

- `build.sh`: `micropython.wasm: 249874 bytes, sha256 590822a7…3b40`.
- The `grep` of the hash and of `micropython profile: hyalos.spike/1` in `README.md`: found.
- `run.sh`:
  ```
  micropython-triage-0 process: result ok 40 effects 1
  micropython-triage-7 process: result ok 40 effects 1
  micropython-no-notify process: result error AttributeError effects 0
  micropython-raw-effect process: refused UnknownEffect
  micropython-triage-0 kvm: refused KernelFault (process-only, issue #503)
  micropython: ok
  ```
- `git diff --exit-code -- spikes/438/evidence spikes/438/guests`: no change after the re-run.
- Both `cargo fmt --check` lines, the three `clippy` lines and the two `attest` builds: clean.
- The `python_` tests with `HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1`: `test result: ok` in every
  binary; `task-kernel`'s unit tests: 20 passed. The `--list` count: 4.
- `release_switches` with KVM and Landlock required: `test result: ok`.
- `grep '^skipped:'` over the log: nothing.
- `cargo xtask lines`, `hyalos-unsafe`, `docs`: ok.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-04.1 and 3.2: both backends | MicroPython is listed `process`. On KVM every MicroPython run ends `KernelFault`, the default kernel image's limit, recorded as `process-only: micropython — does not fit KVM: …` in `spikes/438/README.md` (Recorded blockers: a runtime that does not fit KVM) | The default kernel image overflows its 2 MiB stack once a guest loop runs about 10,000 iterations (Discoveries). `design.md` (Kernel image spike build) leaves the default image unchanged, and its dispatch is not a profile setting, so neither a spike profile nor a spike layout is the fix. The fix is issue #503 | n |
| 3.1: the host's `clang` and `wasm-ld` | They run in a pinned container (`micropython/Containerfile`: the base image by digest, clang and lld 18.1.3 and Binaryen 108 by version) | The host has no `clang`; a pinned container also makes the build the same on any host with docker. The guest is still built by clang for `wasm32-unknown-unknown` with `wasm-ld`, `-nostdlib`, MicroPython's own string sources (`shared/libc/string0.c`) and its own GC heap, with no Emscripten and no `malloc`. With no floats, no `libm` source is needed | n |
| 3.1: clang and wasm-ld | Binaryen's `wasm-opt --asyncify` also runs on the linked module | MicroPython's NLR needs `setjmp` and `longjmp`. LLVM lowers them only to the exception-handling proposal or to Emscripten's JavaScript, neither of which a profile allows (a Stop condition). Asyncify gives them to plain WebAssembly with no import, so the stop does not trigger | n |

## Decisions

- **MicroPython `v1.29.0`**, the newest release tag on 2026-11-25, commit
  `0fd6c573ea815774668bbb16b8e197c8822368b2`, confirmed published at
  `https://github.com/micropython/micropython`. Recorded in `build.sh` and `spikes/438/README.md`.
- **Toolchain:** clang 18.1.3 (`1:18.1.3-1ubuntu1`), wasm-ld 18.1.3, Binaryen 108, in
  `ubuntu@sha256:562111b88caa79c71391c0c4d3c2114a7d5c35915c20561fd893cb1e07929e6b`. python3 and make
  come from the same release unpinned: they drive MicroPython's build, and the two-build compare
  catches any effect they have on the output.
- **The port** (`spikes/438/micropython/port/`): the minimal ROM level with the compiler, no floats, no
  `sys`, `io`, file system, time, REPL or keyboard interrupt, MPZ long integers, `bytearray`, slices.
  A 2 MiB GC heap that is never collected: WebAssembly locals are outside linear memory, so a
  conservative scan of the shadow stack would miss live pointers. A run that fills the heap
  ends `MemoryError`, the same way every time. MicroPython's qstr hashes are fixed by its own source, so no
  seed is needed. Records 1 and 2 are read into 16 KiB buffers; one too large ends the run
  `error BufferTooSmall`.
- **NLR through Asyncify** (`port/sjlj.c`): `setjmp` unwinds the stack to `run` and rewinds it at once,
  keeping the snapshot in a 256 KiB arena; `longjmp` unwinds to `run`, which rewinds the snapshot of
  the buffer it names. NLR buffers nest last in, first out, so each snapshot starts where the
  enclosing one ends. `asyncify-imports` names no real import, so no host call can unwind; only `run`
  is uninstrumented. Asyncify runs before `-Os`.
- **`run`** calls its body through a `volatile` function pointer, so clang cannot judge the body
  `noreturn` and compile away the loop that resumes an unwind.
- **`expected.tsv`'s columns:** id, guest, contract, SDK, script, data file, seed, backends, outcome, record
  (`-` for none), tab-separated, one `#` header line, paths from the repository root. A result
  outcome is `result <bytes> effects <n>`, with the bytes as text. The runtime a `process-only:` line
  names is the guest's file stem.
- **S-04.2.e1 runs under the `no-notify` contract with the SDK generated from it**, the pair a task
  author would have. The script's `AttributeError` comes from the SDK before any host call, so the
  monitor's refusal is never reached.
- **`run.sh`** reads its cases from `expected.tsv` and never writes it. On a host with KVM it also
  checks that the triage case ends `KernelFault` there, so a fix for #503 makes it fail.
- **The S-04.4.e2 copy** adds an imported mutable `i32` global. A function import would renumber
  every function and make the module invalid, which would be refused `InvalidModule` before the
  import check. Every global in the guest is a mutable `i32`, so the copy validates.

## Discoveries

- **The default kernel image overflows its stack on long guest loops (issue #503, P1).** `wasmi`'s
  build script turns on tail-call dispatch for any x86_64 build at opt-level 3, but in the kernel
  image's build the dispatch does not become sibling calls. The native stack grows with each
  executed back edge and runs off the 2 MiB kernel stack (`rsp=0x1e00000`, `cr2=0x1dffff8` at the
  shutdown), which the monitor reports as `KernelFault`. A one-loop C guest ends `KernelFault` at
  10,000 iterations and runs at 5,000; the process backend runs both. Building the kernel image with
  `portable-dispatch` and nothing else changed fixes it: the MicroPython triage run then gives the
  process backend's result on KVM, in about 280 ms. The digest guest (about 30,000 fuel) and the
  existing KVM tests are too short to reach it. Slice 4 should expect the same for Monty, and
  Slice 5's findings ADR should carry it.
- MicroPython's own `nlr_jump` is declared `noreturn` while this `longjmp` returns during the unwind
  (clang warns once, in `py/nlrsetjmp.c`). Asyncify's check right after each call returns before the
  `unreachable` that follows, so it works, as `try`/`except` and every error case show.
- The guest exports Asyncify's five control functions next to `run` and `memory`. The loader allows
  extra exports, and the host calls only `run`.
- MicroPython's start-up and the SDK's compile take about 260,000 fuel before the script's first
  line, and the triage run about 33 million. Slice 5's measurements should use those figures.
