# PRD — hyalOS: a signed Python task with a contract-generated SDK

## Problem

hyalOS runs a task as a WebAssembly guest on four calls, and records every run so a stranger can
replay it. Capsules (`hyalos-kernel-capsules`) will fix the signed contract that says what a task may
do, and it must know first whether that contract can carry a short Python script as its task. Today
nothing answers that:

1. **A task is hand-written WebAssembly.** The only guest is the digest guest in Rust
   (`hyalos-os/guest/src/lib.rs`). Nobody has built a Python interpreter as a hyalOS guest, so its
   size, start-up cost, fuel and determinism on `wasmi` are unknown.
2. **The monitor has no contract.** It enforces fixed limits only: schema `0`, effect kind `0`, one
   effect, size limits (`hyalos-os/audit/src/replay.rs:118`, `:144`). A guest whose SDK leaves out
   `notify` can still call `effect`, so an SDK that hides a method is not a boundary.
3. **A script cannot be an input.** The monitor's input loader splits the inputs file on newlines
   (`hyalos-os/monitor/src/main.rs:182`), so a multi-line script, its SDK or a contract cannot travel
   as one record.
4. **The candidates do not build for hyalOS.** Per the parent brief (`docs/plans/hyalos-kernel/pack-briefs.md:78-85`), MicroPython's
   WebAssembly port uses Emscripten and Monty's
   worker targets `wasm32-wasip1` and needs floats, more than 4 MiB of memory and `getrandom`. The KVM
   backend refuses any guest over 1 MiB (`hyalos-os/monitor/src/kvm.rs:33`) and runs only the
   profile its kernel image was built with (`hyalos-os/monitor/src/main.rs:165`).
5. **A runtime adds an attack surface inside the guest.** A bug in the interpreter cannot escape the
   guest, but hostile input that exploits it could choose a task's answer or the body of its one
   effect, and replay would reproduce that faithfully. Nobody has measured what each runtime lets
   input steer.

## Users and jobs

- **Task author:** writes the work a hyalOS task does. Today that means WebAssembly; the spike asks
  whether it can be Python.
- **Customer and auditor:** authorises a task and later checks its record, alone.
- **Capsules planner:** the developer who fixes the contract in `hyalos-kernel-capsules` and needs
  this spike's answer first.

**Jobs.**

- Task author: "When I write a triage task, I want to write a few lines of Python against an SDK
  that offers only what my contract grants, so I can't reach for a client, a key or a call I wasn't
  given."
- Customer and auditor: "When I check a Python task's record, I want the same replay guarantee a
  hand-written guest gives, so I can trust the run without trusting the interpreter's authors."
- Capsules planner: "When I fix the contract, I want measured evidence on each runtime, so I can
  adopt one, or rule both out, without guessing."

## Goals

- G1. The monitor, not the SDK, refuses any schema or effect a task's contract does not grant, on
  both backends.
- G2. A contract deterministically yields a Python SDK that exposes only the granted calls, and the
  contract, the SDK and the script are all part of the task's identity.
- G3. The same triage script runs unchanged under each candidate runtime that builds, on both
  backends, and every run replays byte-identically.
- G4. Each runtime's cost and attack surface are measured and recorded: guest size, cold start, run
  time, fuel, and what hostile inputs can steer.
- G5. A findings ADR names the runtime capsules should adopt, or states that neither is ready.

## Non-goals

- CPython on WASI: it needs a deterministic WASI layer in the monitor.
- Muru: a server for long-running web apps, not a capsule runtime.
- The real provider SDKs: they need libraries neither runtime can load.
- Replacing the microVM substrate, or making hyalOS a v1 `Substrate` (parent ADR-003).
- The capsule contract itself: the spike's contract is a draft, its check is minimal, and capsules
  replaces both.
- Any change to a release build of the monitor, the kernel image or a record a release writes. All
  of it sits behind the `spike-contract` feature (ADR-001).
- New guest calls. The four calls of `hyalos.abi/1` stay as `ABI.md` states them.
- A second question schema or effect kind on the host. The contract grants a subset of what the host
  already answers.
- Signing the task identity: the spike binds the contract, SDK and script into it (S-02.4, S-03);
  signing it is capsules' concern, and the findings ADR says so.

## Success measures

- M1. `spikes/438/evidence/` holds a record of the triage task under each runtime that built, with
  its replay passing on the process backend and on KVM.
- M2. The spike's hostile-guest suite (raw imports, ungranted SDK calls) refuses every case on both
  backends, and its results are committed.
- M3. A measurements table gives guest size, cold start, run time and fuel for each runtime on each
  backend, with the command that produced each figure.
- M4. The findings ADR is accepted and names a runtime or states that neither is ready, and capsules'
  brief cites it.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | none as a target: the spike measures guest size, cold start, run time and fuel (S-06.2) and records them. A runtime that runs out of the spike profile's fuel on the fixture is a finding, not a failure |
| Reliability | Every committed record replays byte-identically on both backends, run twice |
| Security | The monitor refuses every ungranted schema and effect on both backends (S-01); no release build gains the contract check, a spike profile or a raised limit (S-01.4); hostile inputs are recorded per runtime (S-06.1) |
| Privacy | none: inputs are fixture issue titles, and the spike runs only the stand-in model |
| Accessibility | none: no user interface |
| Observability | none beyond what the monitor already prints for a run: `ok:` with inputs, inferences, effects and fuel |
| Compatibility / portability | Every existing record and fixture under `hyalos-os/` still audits and replays unchanged, with and without the feature |
| Maintainability | hyalOS stays within its 12,000-line budget (`cargo xtask lines`); `unsafe` only in listed files (`cargo xtask hyalos-unsafe`); the interpreter ports and the generator live under `spikes/438/`, outside the budget |

## Assumptions

| Assumption | How to validate | If false |
|---|---|---|
| MicroPython can be built for `wasm32-unknown-unknown` with a custom port that has no Emscripten, no floats and no host imports beyond `hyalos` | Slice building the MicroPython guest; its loader reports no `BadImport` or `InvalidModule` | Record why in the findings; MicroPython is "not ready" |
| A MicroPython guest fits in 1 MiB and 4 MiB of memory | Its size and a run under `hyalos.spike/1` | It runs under a later spike profile with raised limits, and a guest over 1 MiB under a spike-only KVM layout (`GUEST_MAX` is a monitor constant), recorded as a finding |
| Monty can be built for `wasm32-unknown-unknown` with its randomness seeded from a constant | The Monty guest's import list has only `hyalos` calls | Record the blocker; Monty is "not ready" |
| `wasmi` 2.0.3 builds with deterministic float results (canonical NaNs), as parent ADR-002 requires before floats are allowed | Build the spike kernel image and monitor with `wasmi/deterministic` (ADR-007); a NaN-producing guest gives the same transcript on both backends, and profiles 1 and 2 keep their pinned transcripts with it on (Slice 4, 4.3 and 4.4) | Monty cannot run deterministically; record it as Monty's blocker and keep floats refused |
| Enabling a `wasmi` feature for the spike build does not change fuel or transcripts for profile 1 or 2 runs | The existing pinned-transcript tests pass under `--features spike-contract` | Hard stop (Slice 4, ADR-007): the slice ends `blocked` and floats stay refused; the setting is build-wide, so it cannot be kept out of builds that run profiles 1 and 2 |
| The KVM machine can be given enough memory and guest space for Monty under a spike-only layout | The Monty guest boots and runs on KVM under the spike feature | Monty runs on the process backend only, recorded as a finding (and criterion S-05.1 records the gap) |
| Monty's type checking can run over a script with a stub of the generated SDK | Run it against a script calling an ungranted method | Recorded; it does not fail the spike (S-05.3) |

## Story map

| Bound the task | Describe the task | Run it as Python | Judge it |
|---|---|---|---|
| S-01 | S-02, S-03 | S-04, S-05 | S-06 |

## Stories and acceptance criteria

### S-01 The monitor enforces a draft contract

As a customer, I want the monitor to refuse any question or effect a task's contract does not grant,
so that hiding a method in the SDK is never what keeps a task in bounds.

- **S-01.1** Given a run under a spike profile whose contract grants schema `0` and no effect, when
  the guest calls `effect`, then the run ends `UnknownEffect` on both backends and no effect reaches
  the source.
- **S-01.2** Given a run under a spike profile whose contract grants effect kind `0` and no schema,
  when the guest calls `infer`, then the run ends `UnknownSchema` on both backends and no question
  reaches the source.
- **S-01.3** Given a run under a spike profile whose contract is missing, unreadable, empty or not a
  contract, when the run
  starts, then it is refused before the guest loads, with `BadContract`, on both backends.
- **S-01.4** Given a build without `spike-contract`, when anything names a spike profile, then that
  profile is unknown (a record naming it is refused as an unknown profile) and every existing pinned
  transcript is unchanged.
- **S-01.5** Given a record under a spike profile, when it is replayed, then the contract is read from
  the record's own inputs and enforced again, so a record whose answers or effects exceed its
  contract fails replay.

### S-02 A contract yields its SDK

As a task author, I want an SDK generated from my contract with only the calls it grants, so that the
script sees no client, key, model setting, URL or tool list.

- **S-02.1** Given a contract, when the generator runs twice, then it writes the same SDK bytes.
- **S-02.2** Given the draft contract (one classification schema, one notification), when the SDK
  is generated, then its module `hyalos` exposes only `task`, whose public members are
  `read_inputs`, `ask` (with `classify`), `notify` (with `team`) and `finish`, and nothing else.
- **S-02.3** Given a contract that omits the notification, when the SDK is generated, then it has no
  `notify` member at all.
- **S-02.4** Given two contracts that differ in one grant, when each SDK is generated and the same
  script is run under each, then the SDK bytes differ and the two runs' task identities differ.

### S-03 The script, the SDK and the contract travel as whole records

As a customer, I want a Python task's script, SDK and contract bound into the task identity I
authorise, so that a changed script or SDK is a different task.

- **S-03.1** Given a contract, an SDK and a multi-line script, when the spike's run command reads
  them, then each reaches the guest as one input record with every byte, newlines included, ahead of
  the data records.
- **S-03.2** Given a run under a spike profile, when its record is written, then its task identity is
  the existing version 4 framing over the guest hash and all input records, contract, SDK and script
  first, and no new record field or version is introduced.
- **S-03.3** Given a record, when one byte of its script record changes, then the record's task no
  longer matches and `read_transcript` refuses it.

### S-04 The triage task runs under MicroPython

As a task author, I want the triage task in Python to run under MicroPython as a hyalOS guest, so
that capsules can judge it on evidence.

- **S-04.1** Given the MicroPython guest, the generated SDK and the triage script, when it runs on
  the issue fixture on each backend, then both give the same transcript, and replay passes on both.
- **S-04.2** Given a script that calls a method the SDK does not expose, when it runs, then the
  script fails inside the guest with Python's attribute error, the ungranted call reaches no host
  call (no `infer` or `effect`), and the transcript records none for it. Granted calls the script
  made before it (such as `ask.classify`) are recorded as usual.
- **S-04.3** Given a hostile guest script that reaches the native `_hyalos` module directly, bypassing
  the SDK, when it calls an ungranted schema or effect, then the monitor refuses it as S-01 states.
- **S-04.4** Given the MicroPython guest, when its imports are listed, then they are only the four
  `hyalos` calls.

### S-05 The triage task runs under Monty

As a task author, I want the same script and SDK to run under Monty, so that the two runtimes are
compared on one task.

- **S-05.1** Given the Monty guest under its spike profile, the same SDK and the same script, when it
  runs on the issue fixture on each backend, then both give the same transcript, and replay passes
  on both. If Monty does not build, or cannot run on KVM, the reason is recorded and this criterion
  is met by that record.
- **S-05.2** Given a script that calls an ungranted method, and a hostile script that reaches Monty's
  host functions directly, when each runs, then the first fails in the guest, its ungranted call
  reaching no host call, and the second is refused by the monitor, as S-04.2 and S-04.3.
- **S-05.3** Given Monty's type checking and a stub of the generated SDK, when it checks a script that
  calls an ungranted method, then the result (rejected, accepted or unusable) is recorded. Any result
  meets this criterion.

### S-06 The findings

As the capsules planner, I want each runtime measured and attacked the same way, so that I can choose
one, or neither, on evidence.

- **S-06.1** Given a hostile-input fixture, when the triage task runs on it under each runtime, then
  every case runs to a result or a refusal, and what each case could steer (labels asked, effect
  body, result, fuel) is recorded per runtime.
- **S-06.2** Given each runtime that built, when it runs the issue fixture, then its guest size, cold
  start, run time and fuel on each backend are recorded with the command that measured them.
- **S-06.3** Given the evidence, when the findings ADR is written, then it names the runtime capsules
  should adopt, or states that neither is ready, and states the hostile-input limit an audit report
  must carry for any Python task.

## Rules and examples

### S-01 The monitor enforces a draft contract

Rule: under a spike profile, a call passes the host's checks only if the contract grants its schema
or effect kind; the reason is the existing one for an unknown schema or effect.

| ID | Given contract | When the guest calls | Then, on both backends |
|---|---|---|---|
| S-01.1.e1 | grants schema 0, no effect | `effect(0, …)` | `UnknownEffect`; the source saw no effect |
| S-01.1.e2 | grants schema 0 and effect 0 | `effect(0, …)` once | the effect is recorded |
| S-01.2.e1 | grants effect 0, no schema | `infer(0, …)` | `UnknownSchema`; the source saw no question |
| S-01.2.e2 | grants schema 0 | `infer(9, …)` | `UnknownSchema`, as without a contract |

Rule: a spike run without a readable contract never starts.

| ID | Given | When the run starts | Then |
|---|---|---|---|
| S-01.3.e1 | input record 0 is not a contract | on either backend | `BadContract`, before the guest loads |
| S-01.3.e2 | no input records at all | on either backend | `BadContract` |
| S-01.3.e3 | a contract with an unknown field | on either backend | `BadContract` |

Rule: a build without the feature knows no spike profile, and its pinned transcripts are unchanged.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.4.e1 | a default build | a record naming `hyalos.spike/1` is read | refused as an unknown profile |
| S-01.4.e2 | a build with `spike-contract` | the digest guest runs under `hyalos.profile/2` | the pinned transcripts and fuel hold: `core_s_02_1_e1`'s for profile 1, and the profile 2 pin Slice 1 takes at the base commit (Slice 1, 1.6) |

Rule: replay enforces the record's own contract.

| ID | Given a spike record | When replayed | Then |
|---|---|---|---|
| S-01.5.e1 | as written | on both backends | passes |
| S-01.5.e2 | its contract record edited to drop effect 0, task recomputed | on both backends | fails with `UnknownEffect` |

### S-02 A contract yields its SDK

Rule: generation is a pure function of the contract's bytes.

| ID | Given | When generated | Then |
|---|---|---|---|
| S-02.1.e1 | the draft contract | twice | identical bytes |
| S-02.1.e2 | the draft contract with its fields reordered | once each | the generator refuses any but the canonical order, so no two byte forms give one SDK |

Rule: the SDK exposes exactly the granted calls.

| ID | Given contract | When the SDK's public names are listed | Then |
|---|---|---|---|
| S-02.2.e1 | the draft contract | — | the module: `task`; `task`: `read_inputs`, `ask.classify`, `notify.team`, `finish` |
| S-02.3.e1 | no notification | — | no `notify` |
| S-02.3.e2 | no schema | — | no `ask` |

Rule: a different contract is a different task.

| ID | Given | When run with the same script | Then |
|---|---|---|---|
| S-02.4.e1 | the draft and the no-notification contract | under the same runtime | SDK bytes differ, task identities differ |
| S-02.4.e2 | the same contract | twice | same SDK bytes, same task identity |

### S-03 The script, the SDK and the contract travel as whole records

Rule: the spike's run command delivers each file as one record, unsplit.

| ID | Given | When the run starts | Then the guest's records are |
|---|---|---|---|
| S-03.1.e1 | a 40-line script and a 3-line data file | — | 0 contract, 1 SDK, 2 the whole script, 3 to 5 the data lines |
| S-03.1.e2 | an empty script file | — | refused before the run: a spike run needs a script |

Rule: identity covers every record with the existing framing.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.2.e1 | a spike run | the record is written | version 4, `task` equals `task_v4` over the spike profile, guest hash and all records |
| S-03.3.e1 | that record with one script byte changed | `read_transcript` | refused: the task is not the one its guest and inputs give |

### S-04 The triage task runs under MicroPython

Rule: one script, one SDK, both backends, byte-identical.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.1.e1 | the issue fixture, seed 0 | run on each backend | identical transcripts; replay passes on both |
| S-04.1.e2 | the issue fixture, seed 7 (some answers change) | run on each backend | identical transcripts to each other, different from seed 0's |

Rule: an ungranted SDK call fails inside the guest; a raw call is refused by the monitor.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.2.e1 | the no-notification SDK and the triage script | run | Python attribute error; no `effect` (the `infer` calls for `ask.classify` before it are recorded); the run ends with `finish` of `error AttributeError` (`design.md` §2, Runtime guest `run`), recorded |
| S-04.3.e1 | the no-notification contract and a script calling the native `_hyalos.effect` | run on each backend | `UnknownEffect` |
| S-04.4.e1 | the MicroPython guest | its imports listed | the four `hyalos` calls only |
| S-04.4.e2 | a build that leaks one other import | loaded | `BadImport` |

### S-05 The triage task runs under Monty

Rule: the same evidence as MicroPython, or a recorded reason why not.

| ID | Given | When | Then |
|---|---|---|---|
| S-05.1.e1 | the Monty guest under its spike profile, issue fixture, seed 0 | run on each backend | identical transcripts; replay passes on both |
| S-05.1.e2 | Monty does not build for `wasm32-unknown-unknown`, or does not fit KVM | — | the blocker is recorded in the findings, with the command that shows it |
| S-05.2.e1 | the no-notification SDK | the triage script runs | an error in the guest; no `effect` (the `infer` calls before it are recorded) |
| S-05.2.e2 | a script calling Monty's host function for `effect` directly under the no-notification contract | run on each backend | `UnknownEffect` |

Rule: type checking is tried and its result recorded, whatever it is.

| ID | Given | When | Then |
|---|---|---|---|
| S-05.3.e1 | a script calling `notify.team` against the no-notification SDK stub | type-checked | the result is recorded |
| S-05.3.e2 | type checking cannot run on a script for this SDK | — | "unusable", with why, is recorded |

### S-06 The findings

Rule: every hostile input reaches a result or a refusal, and its reach is recorded.

| ID | Given | When run under each runtime | Then |
|---|---|---|---|
| S-06.1.e1 | a title of invalid UTF-8 | — | a result or a refusal; what it changed is recorded |
| S-06.1.e2 | a record of exactly 16,384 bytes (the guest's read buffer), and one of 16,385 bytes | — | a result or a refusal; recorded |
| S-06.1.e3 | 200 titles crafted to collide in the runtime's string hashing (where that hash cannot be reproduced, 200 same-length titles, and collisions are recorded as untested) | — | fuel recorded; `OutOfFuel` is an acceptable, recorded outcome |

Rule: every figure has its command.

| ID | Given | When | Then |
|---|---|---|---|
| S-06.2.e1 | a runtime that built | measured on both backends | size, cold start, run time and fuel recorded with commands |
| S-06.2.e2 | a runtime that did not build | — | its row says why, with no figures |
| S-06.3.e1 | both runtimes measured | the findings ADR | names one, or neither, with reasons |
| S-06.3.e2 | neither runtime built | the findings ADR | states neither is ready and what each needs |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| A release build gains the contract check or a spike profile | the feature leaks into a default build | `tests/release_switches.rs`-style check fails | CI's `hyalos-os` job fails | S-01.4 |
| The spike's `wasmi` setting changes an existing profile's fuel | enabling deterministic floats workspace-wide | pinned-transcript tests fail under the feature | the test names the profile and digest | S-01.4 |
| A record exceeds its contract but replays | the contract is not re-read in replay | replay fails `UnknownSchema` or `UnknownEffect` | the audit refuses the record | S-01.5 |
| A script's bytes are split | the newline loader is used for scripts | the guest gets a fragment; its run fails | a test on S-03.1.e1 fails | S-03.1 |
| An interpreter is nondeterministic | hash seeding, address-dependent ordering, time or randomness | the two backends' transcripts differ | `on_both`-style comparison fails, naming the runtime | S-04.1, S-05.1 |
| A runtime cannot be built or run | toolchain or upstream blocker | recorded in `spikes/438/README.md` by its slice, and carried into the findings ADR by Slice 5; not worked around | the findings row says why | S-05.1, S-06.2 |
| A hostile input steers the effect body | an interpreter bug | recorded as reach, not fixed | the hostile-input table lists it | S-06.1 |
