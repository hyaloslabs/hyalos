# PRD — hyalOS: the task contract

## Problem

The Python spike (#438) showed that a short MicroPython script can be a hyalOS task, but only
under spike profiles, a draft contract and a feature that no release build carries. Nothing yet
lets a customer authorise a real Python task with stated bounds. The causes:

1. **MicroPython does not run on KVM.** The default kernel image overflows its 2 MiB stack at about
   10,000 guest loop iterations and ends `KernelFault`, and MicroPython's start-up alone runs past
   that (#503). Every MicroPython case in the committed evidence is `process` only (context §2 A).
2. **A script cannot be an input under the real profile.** `read_inputs` splits the inputs file on
   `\n` and drops empty lines (context §2 B), and `hyalos.abi/1` says each input record is one line
   with no newline. Only the spike's own command carries the contract, SDK and script as whole
   records, and only under a spike profile (context §2 B, `spike.rs`).
3. **The contract is a draft.** It holds `asks` and `may_touch` only, grants schema `0` and effect
   `0` only, and is checked only with `spike-contract` under a `hyalos.spike/` profile (context
   §2 C). No contract states what a task accepts, returns or may spend.
4. **A question has one answer type.** `infer` returns one of four labels for schema `0`, and the
   record, the monitor, the attest tool, the notary, the verifier's check and the canary each hold a
   copy of that one template (context §2 D, §3). A task cannot ask a yes-or-no question, a number or a choice of its own.
5. **Nothing bounds how many questions a run asks.** `pre_infer` checks no count, and the notary
   has no cap on sessions per run (context §2 E). Nothing has measured what an attested call costs
   at the largest input a task accepts: one real call has been measured, 7.42 s over two machines,
   and 1.9 s with both sides in one process (context §2 E).
6. **Hostile data can exhaust MicroPython's heap.** The heap is never collected (context §2 A,
   `port/main.c`), so a valid-looking input can end the run `MemoryError` with no notification
   (Python ADR-008, hostile-input limit). No contract states input limits that prevent this.
7. **The audit report does not state Python ADR-008's limit.** It takes no input that marks a guest as a
   runtime guest, and its wording is pinned by golden reports (context §2 G).

## Users and jobs

- **Customer:** authorises one exact task, inputs included, and runs the notary (record ADR-002,
  context §1).
- **Auditor:** anyone who checks a record alone, with the verifier and the audit.
- **Task author:** writes a task's Python script against the SDK its contract yields.
- **Operator:** runs the monitor on KVM or as a process, and submits the record.
- **Verifier author:** writes or maintains a program that reads `hyalos.abi/N` and the record
  versions.

**Jobs.**

- Customer: "When I authorise a task, I want its contract to state what it may ask, touch and
  return, its limits and its most questions, bound into the identity I sign, so I can know what I
  approved without trusting the operator."
- Auditor: "When I check a Python task's record, I want it to replay byte-identically on either
  backend and the report to say what it does not prove, so I neither over-trust nor under-trust it."
- Task author: "When I write a script, I want an SDK that offers exactly my contract's questions,
  each with its answer type, and nothing else, so I can't call what I wasn't granted."
- Operator: "When I run a Python task, I want it to run on KVM as it does as a process, so I can
  use the confined backend for every task."
- Verifier author: "When the interface or record changes, I want a new named version with its own
  pinned section, so every record I already read stays valid and the new one is explicit."

## Goals

- G1. Long guest loops, MicroPython's included, run on the KVM backend, and MicroPython's committed
  records replay byte-identically on both backends.
- G2. Input records are length-framed under `hyalos.abi/2` and a new record version, so the
  contract, SDK and script travel as whole inputs under the real profile and the task identity
  covers every byte.
- G3. The monitor enforces the real contract on both backends and in replay, replacing the spike's
  check.
- G4. A question's answer is one of three closed types, `bool`, a bounded integer or a choice from
  a declared list, and the model never returns free text.
- G5. A run asks at most its contract's declared question limit, within a ceiling set from
  measured costs against the real provider.
- G6. A contract deterministically yields its SDK, from a generator kept outside the monitor.
- G7. The audit report for every contract task states Python ADR-008's hostile-input limit.

## Non-goals

- Real connectors (the connectors pack).
- Any chain a model assembles at run time.
- Work that spans tasks: v1's workflow under parent ADR-003, for the engine pack. Multi-step work
  is one task's script under one authorisation and one record.
- Signed chains as their own construct.
- A looser authorisation for new inputs. One authorisation per run names the exact task and its
  inputs (record ADR-002).
- A signature or key on the contract itself. The customer's authorisation of the task identity
  binds it.
- Free-text model answers.
- New float or memory profiles. Monty is not adopted (Python ADR-008).
- A contract for native guests designed first, or a contract design without MicroPython.
- Type checking before signing. The monitor's check is the boundary (Python ADR-008).
- Building a second, non-GitHub job. Spec names what one would need from the contract's fields, as
  a check only.
- Policy admission of a signed contract, or the log event linking a record (engine pack).
- Editing the `hyalos.abi/1` section, or changing how any version 1 to 4 record reads, audits or
  replays, apart from the two refusals S-02.5 and S-02.6 add.
- Raising the 12,000-line budget without a new ADR (ADR-0133).
- Auditing a task that asks no question. Its record has no attested answer and no ticket, so the
  audit refuses it as today; making it auditable needs the customer's authorisation proven without
  a notary session, a new trust path (ADR-007).
- A zero-capacity answer channel. Closed types bound what a model can pass to a script, not to
  nothing: up to 32 bits per integer answer, so about 800 bytes over 200 questions (ADR-018).
- A notary that caps sessions at the contract's limit. The monitor and the audit enforce it
  (ADR-007).
- Detecting whether a guest is a language runtime (ADR-009).

## Success measures

- M1. A committed MicroPython triage record under the real contract and `hyalos.profile/2` replays
  byte-identically on both backends in CI, with `HYALOS_REQUIRE_KVM=1` set.
- M2. `spikes/438/evidence/expected.tsv` lists no MicroPython case as `process` only, and
  `spikes/438/README.md` holds no `process-only: micropython` line.
- M3. A committed measurement for #411 gives, against the real provider and at the largest input
  the triage contract accepts, the call time and bytes per attestation, each with the command that
  produced it and the network path it took; the notary's concurrency is taken from the notary's
  code and its busy test, not a live two-session run (ADR-034, ADR-036).
- M4. `hyalos-os/ABI.md` holds a `hyalos.abi/2` section with its own pinned digest, and the
  `hyalos.abi/1` digest is unchanged.
- M5. The hostile-guest suite refuses every ungranted schema, effect and over-limit question on both
  backends under the real profile. An out-of-type live answer is refused `ModelRefused` on the
  process backend (S-06.3), and an out-of-type recorded answer fails replay on both backends (S-06.4).
- M6. A golden report for a Python task carries Python ADR-008's limit, and every existing golden report
  is unchanged.
- M7. `cargo xtask lines` stays at or under 12,000, or a new ADR records why not.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | The question-limit ceiling and the live deadline rest on the #411 measurement at the largest accepted input (S-08); a live run that asks its full limit at the measured call time ends before its deadline (S-08.3). The #503 fix leaves `fuel_used` unchanged for every committed profile 1 and 2 record (S-01.3) |
| Reliability | Every committed record of the new version replays byte-identically on both backends, run twice; with fixed inputs and seed, a re-run writes the same record (S-01.2, S-03.4) |
| Security | The monitor refuses every ungranted schema and effect, every question past the limit, every answer outside its type and every input outside `accepts`, on both backends and in replay (S-04 to S-07). No release build knows a spike profile (S-04.5). The design gets an adversarial read before slicing (README Open questions) |
| Privacy | none: the contract adds no new data source; inputs and answers are what the record already carries, and the notary sees no plaintext (notary pack) |
| Accessibility | none: no user interface; the audit report's wording is pinned text (S-10) |
| Observability | Every new refusal has a name listed in `ABI.md` and the closed `Reason` set (context §3); the audit report names the limit it relies on (S-10) |
| Compatibility / portability | Every version 1 to 4 record and every existing golden report reads, audits and replays unchanged (S-02.5), apart from the two refusals S-02.5 and S-02.6 add; `hyalos.abi/1`'s section and digest are unchanged (S-02.4); a record of the new version names `hyalos.abi/2` (S-02.3) |
| Maintainability | hyalOS's non-test Rust stays within 12,000 lines (`cargo xtask lines`), 2,653 remaining at `bf09657`, about +545 net estimated (design.md, Line budget estimate), or a new ADR says why not; `unsafe` only in listed files (`cargo xtask hyalos-unsafe`); the generator lives outside the monitor (S-09.3) |

## Assumptions

| Assumption | How to validate | If false | Probe |
|---|---|---|---|
| Building the kernel image with `portable-dispatch` and nothing else changed removes #503 (context §5, #503) | Build the image that way; run a one-loop guest at 10,000 and 1,000,000 iterations and the MicroPython triage case on KVM | The fix grows the kernel stack instead (context §2 A, `kvm.rs`), or needs its own ADR | not run: changes state (needs the image built and a KVM run). `portable-dispatch` exists and overrides `auto-dispatch` in `wasmi` 2.0.3; the fix rests on one local run recorded in python `slice-03.md:126` (`docs/plans/hyalos-kernel-python/progress/slice-03.md:126`) |
| That build leaves transcripts unchanged for every pinned profile 1 and 2 transcript, and fuel wherever it is pinned (profile 1's `BASE` pins seeds and hashes; profile 2's `PROFILE_2_BASE` pins hashes and fuel) | Run the pinned-transcript and committed-evidence tests on both backends with the rebuilt image | Hard stop: a profile's replay would change, which needs a new profile name (parent ADR-002) | not run: needs the rebuilt image; the pins to run against are `ring_tests.rs:15` and `spike_tests.rs:13` |
| MicroPython runs the triage task within `hyalos.profile/2`'s fuel (200,000,000) and 4 MiB memory at the largest input the triage contract accepts (Python ADR-008, MicroPython and "What capsules must decide") | Run the triage task on the largest accepted input on both backends and read `fuel_used` | `record_bytes` is set by the outcome search and its margin, min(4,096, floor(0.75 × R_max)), down to a floor of 512 (ADR-033), with `records` staying 200 until Slice 11, which lowers it to the ceiling if that is below 200 (ADR-027), since a new memory or fuel profile is ruled out | not run: needs the real contract and its `accepts`; profile 2's 200,000,000 fuel and 4 MiB confirmed (`profile/src/lib.rs:50-51`) |
| The version 4 task identity already frames every input with its length (context §2 B, §3), so framing changes the inputs file form and not the identity's coverage | Compute the task identity for the same records read from a framed file and from the spike's records | The new record version needs a new identity function | held: `framed_task` frames the input count and each input's length and bytes, over `&[Vec<u8>]`, not a file (`hyalos-os/audit/src/record.rs:260-280`) |
| The KVM backend's limits fit the contract, SDK, script and largest accepted data: `RECORD_MAX` 16 MiB per record and a 1 MiB guest slot (context §2 A, §2 B) | Run the largest accepted task on KVM | `accepts` is narrowed, or the layout changes under its own ADR | held for the constants (`kvm.rs:323` 16 MiB, `kvm.rs:33` 1 MiB; the guest is 249,874 bytes); the largest-task run not run: needs the contract |
| The real provider honours a structured answer schema for a boolean, a bounded integer and an enum, so a closed type can be enforced in the attested request (context §2 D, `attest/src/lib.rs`) | Send one attested request of each type to the real provider and read the reply | The notary or monitor must check the type after the reply, and an out-of-type reply is refused (S-06.3) | not run: needs the network and a credential; today's template sends a string enum only (`attest/src/lib.rs:146-165`) |
| Under notary ADR-014, a session opened past the last answer cannot help a record pass, so the audit counting a record's answers against its limit is enough to stop a re-roll | Read ADR-014's Consequences and `hyalos-os/audit/src/answers.rs` with a record that holds one more answer than its limit | The notary must also refuse sessions past the limit, against ADR-007 | held, by reading: ADR-014 Consequences, and `answers.rs:106-108` requires each sequence to equal its position, so the audit must count answers itself; the extra-answer run not run: needs a contract limit |
| The notary's one-session-at-a-time rule (context §2 E, `Semaphore::new(1)`) is the concurrency the #411 measurement must state | Read `hyalos-os/attest/src/bin/notary.rs:1320`, `:1328-1332` and its busy test `t_link_a_second_connection_during_a_session_is_refused_busy` (`hyalos-os/attest/tests/notary.rs:684-701`); no live two-session run (ADR-034, ADR-036) | The measurement states the concurrency from the notary's code and busy test | held, by reading: `notary.rs:1320` `Semaphore::new(1)`, `:1329-1332` refuses busy; the concurrent run not run: starts a notary |
| The audit can tell a Python task's record from a native guest's from the record alone, through its contract (context §2 G) | Read a record of each kind and the contract's fields | The report states the limit for every contract record (ADR-009) | false: `audit_report` takes six arguments (`Audited`, the answer count, the test-settings flag, the attempt, the proof count and the oldest pass, `hyalos-os/audit/src/report.rs:22-29`), and none marks a runtime; the record names a profile and a guest hash; no default-build record carries a contract (`report.rs:1-103`, `profile/src/lib.rs:130-135`). Settled by ADR-009 |
| The work fits in the 2,653 lines left under the budget | Estimate per slice at spec, then `cargo xtask lines` after each slice | A new ADR says why the budget moves (ADR-0133) | held for the figure (`cargo xtask lines`: 9347 / 12000, 2,653 left); the fit not run: needs per-slice estimates |

## Story map

| Run on both backends | Carry the task | Bound the task | Ask the model | Write the task | Judge the run |
|---|---|---|---|---|---|
| S-01 | S-02, S-03 | S-04, S-05 | S-06, S-07, S-08 | S-09 | S-10 |

## Stories and acceptance criteria

### S-01 Long guest loops run on KVM

As an operator, I want the KVM backend to run guests with long loops, MicroPython included, so that
every Python task can use the confined backend.

- **S-01.1** Given the default kernel image, when a guest runs a loop of well over 10,000
  iterations on KVM, then it ends with the same outcome as on the process backend, not
  `KernelFault` (context §2 A).
- **S-01.2** Given the committed MicroPython guest, its SDK, the triage script and the issue
  fixture, when it runs on each backend, then both write the same transcript, it equals the
  committed record, and replay passes on both (context §2 A, `spike_evidence_tests.rs`).
- **S-01.3** Given every pinned profile 1 and profile 2 transcript (profile 1's `BASE`,
  `ring_tests.rs:15`, pins seeds and hashes only; profile 2's `PROFILE_2_BASE`, `spike_tests.rs:13`,
  pins hashes and fuel), and every committed record, when each is run or replayed on the rebuilt
  image, then its transcript is unchanged, and so is `fuel_used` wherever it is pinned or recorded.
- **S-01.4** Given the fix, when the spike's evidence is read, then every MicroPython case, the
  eight hostile cases included, is listed `both`; each case that ends with a result has a committed
  record, and a case that is refused gives the same refusal on both backends; and `run.sh` fails if
  the triage case ends `KernelFault` on KVM (context §2 A).

### S-02 Inputs are length-framed under a new interface and record version

As a task author, I want each input record to carry any bytes, newlines included, so that a script,
an SDK and a contract are each one input.

- **S-02.1** Given an inputs file in the framed form, when the monitor reads it, then every record
  reaches the guest with every byte, newlines and empty records included (context §2 B).
- **S-02.2** Given a framed inputs file whose stated length runs past its end, or with bytes after
  the last record, when the monitor reads it, then the run is refused `BadInputs` before the guest
  loads (ADR-008).
- **S-02.3** Given a run under `hyalos.abi/2`, when its record is written, then it is the new
  record version and names `hyalos.abi/2` (core ADR-004, Consequences; context §1).
- **S-02.4** Given `hyalos-os/ABI.md`, when the conformance suite runs, then the `hyalos.abi/1`
  section's pinned digest is unchanged and a new `hyalos.abi/2` section has its own pinned digest
  (context §2 B, §3). The section is committed in Slice 2, marked draft, and pinned in Slice 11.
- **S-02.5** Given any version 1 to 4 record, when it is read, audited and replayed, then the
  result is as before; and a record cannot change version (record ADR-011, context §2 B). A version
  1 to 4 record holding an `abi` field is refused (ADR-018).
- **S-02.6** Given a default command's run that writes version 4 (`run`, `live`, `fuzz`, `task`,
  `attacks` or `bench`), when an input record holds a newline, is empty or is not UTF-8, then it is
  refused `BadInputs` before the guest loads, so the `hyalos.abi/1` input rule holds whatever form
  the file takes. The spike's `spike-run` keeps its own line split until it is removed (ADR-018,
  ADR-020).
- **S-02.7** Given a default build before the commit that pins `hyalos.abi/2`, when a version 5
  record is read, then it is refused as inconclusive (exit 2), since a later build may read it
  (ADR-016, ADR-023).

### S-03 The contract, SDK and script travel as inputs under the real profile

As a customer, I want a Python task's contract, SDK and script bound into the task identity I
authorise, under the profile new runs use, so that any changed byte is a different task.

- **S-03.1** Given a contract, its SDK, a multi-line script and data records, when the run command
  reads them, then the run uses `hyalos.profile/2` and no spike profile, and each reaches the guest
  as one whole record (context §1, §2 B).
- **S-03.2** Given such a run, when its record is written, then its task identity frames the profile
  name, the guest hash and every input byte, the contract, SDK and script included, and `task`
  prints the guest hash beside the identity (context §3; ADR-018).
- **S-03.3** Given a record, when one byte of its contract, SDK, script or data changes, then its
  task no longer matches and `read_transcript` refuses it (context §3).
- **S-03.4** Given fixed inputs and seed, when the task is run twice, then both records are byte
  identical (context §3, stand-in model).

### S-04 The monitor enforces the contract

As a customer, I want the monitor to hold a task to its contract on both backends and in replay, so
that the SDK hiding a call is never what keeps a task in bounds.

- **S-04.1** Given a contract task, when the guest asks a schema its contract's `asks` lacks, then
  the run ends `UnknownSchema` on both backends and no question reaches the source (context §2 C,
  §3).
- **S-04.2** Given a contract task, when the guest requests an effect its contract's `may_touch`
  lacks, then the run ends `UnknownEffect` on both backends and no effect reaches the source
  (context §2 C).
- **S-04.3** Given a run of the new record version whose contract record is missing, unreadable,
  not canonical or holds an unknown field, when the run starts, then it is refused `BadContract`
  before the guest loads, on both backends. Every task of the new version carries a contract, the
  digest guest's included (ADR-006; context §2 C).
- **S-04.4** Given a contract task, when the guest returns a result its contract's `returns` does
  not allow, then the run is refused `ResultLimit` and no result is recorded. `returns` declares a
  size limit only (ADR-008).
- **S-04.5** Given a default build, when anything names `hyalos.spike/1` or `hyalos.spike/2`, then
  the profile is unknown (context §2 C, `profile/src/lib.rs`).
- **S-04.6** Given a contract record, when it is replayed, then the contract is read from the
  record's own inputs and enforced again, so a record whose questions, answers, effects or result
  exceed its contract fails replay (context §3).

### S-05 What a task accepts cannot exhaust the heap

As a customer, I want the contract's `accepts` to state input limits derived from the heap, so
that a valid-looking input cannot suppress the task's one effect with a `MemoryError`.

- **S-05.1** Given a contract task, when its data inputs exceed any limit its `accepts` states, then
  the run is refused `InputLimit` before the guest loads, on both backends (ADR-008).
- **S-05.2** Given the MicroPython triage contract, when the triage task runs on the largest input
  its `accepts` allows, then it ends with a result and its notification on both backends, never
  `MemoryError`, and the SDK skips a data record that is not UTF-8 before asking about it (Python
  ADR-008, hostile-input limit; ADR-018; context §2 A, `port/main.c`).
- **S-05.3** Given the triage contract's `accepts`, when its limits are recorded, then each states
  the outcome search it was derived from (ADR-033), its margin and the command that ran it.

### S-06 Questions take closed answer types

As a customer, I want every answer to be a `bool`, a bounded integer or a choice from a declared
list, so that no model text can carry an instruction into the next step or an effect's body.

- **S-06.1** Given a contract whose `asks` declares a schema of each closed type, when the guest
  asks each, then each answer the guest receives is a value of that schema's type: `true` or
  `false`; an integer within the declared bounds; or one of the declared choices (context §2 D).
- **S-06.2** Given a contract, when it declares an answer type that is not one of the three, a
  bounded integer whose lower bound exceeds its upper or whose bounds fall outside `i32`, or a
  choice list that is empty or repeats a choice, then the contract is refused `BadContract` before
  the guest loads (ADR-004, ADR-008).
- **S-06.3** Given a live run, when the model's reply is not a value of the schema's type, then the
  run ends `ModelRefused` and the reply never reaches the guest (context §2 D, `ABI.md`).
- **S-06.4** Given a record, when a recorded answer is not a value of its schema's declared type,
  then replay fails (context §2 D, `replay.rs`).
- **S-06.5** Given a version 5 record, when a presentation's request is not the closed-type
  template for its schema's declared type and prompt, free text or today's template included, then
  the audit and `hyalos-verify` refuse the record, so no passing answer is free text. The notary
  signs blind and does not check the template (ADR-015; context §2 D).
- **S-06.7** Given the request templates, when one changes, then `RELEASE` changes with it, held by
  a test (ADR-015).
- **S-06.6** Given a notary of another release and a run, when the run opens its first session,
  then the session is refused and no ticket is spent (ADR-007).

### S-07 A per-task question limit

As a customer, I want my contract to declare the most questions a run may ask, and the monitor to
enforce it, so that a run cannot re-roll answers past what I approved.

- **S-07.1** Given a contract with question limit L, when the guest asks its (L+1)th question in
  that run, then the run is refused `QuestionLimit` before the question leaves the monitor, on
  both backends (ADR-007, ADR-008; context §2 E, §3).
- **S-07.2** Given a contract with question limit L, when the guest asks L questions, then every
  one is answered and recorded.
- **S-07.3** Given a record holding more answers than its contract's limit, when it is replayed or
  audited, then it fails.
- **S-07.4** Given a contract that declares no question limit, or a limit above the ceiling, when it
  is read, then it is refused `BadContract` before the guest loads. The ceiling is 200, provisional,
  until S-08.2 replaces it with the measured value (ADR-007).
- **S-07.5** Given a contract task that asks no question, when it runs live with a notary, then no
  session opens and no ticket is spent, and the audit refuses its record as it does today: auditing
  such a task is a non-goal (ADR-007; record ADR-004; context §1).

### S-08 The question-limit ceiling is measured (#411)

As a customer, I want the ceiling set from what attested calls really cost at the largest
input a task accepts, so that it is neither a guess nor a run that cannot finish.

- **S-08.1** Given the real provider and the notary, when the triage task runs on the largest input
  its contract accepts, then the call time and bytes per attestation are measured against the real
  provider and recorded, each with its command and the network path it took, and the notary's
  concurrency is recorded from the notary's code and its busy test (context §2 E; notary pack
  Retro; ADR-034, ADR-036), not measured by a live two-session run.
- **S-08.2** Given that measurement, when the ceiling is set, then an ADR records its value and
  the figures it rests on, replacing the provisional 200 (ADR-007, ADR-010).
- **S-08.3** Given a live run whose contract's limit is L, when its deadline is set, then it is the
  larger of today's `max(600, inputs × 35 + 60)` seconds and L × the measured call time × 1.5, and
  a run that asks L questions at the measured call time ends before it. For version 5, "inputs"
  counts data records only, not the contract or code records (design.md §2.8; ADR-010, ADR-014;
  context §2 E, `main.rs:784`).

### S-09 A contract yields its SDK

As a task author, I want an SDK generated from my contract with exactly its granted calls and answer
types, so that the script sees nothing it was not granted.

- **S-09.1** Given a contract, when the generator runs twice, then it writes the same SDK bytes
  (context §2 F).
- **S-09.2** Given a contract, when its SDK is generated, then it exposes one ask per `asks` schema,
  typed with that schema's answer type, one call per `may_touch` effect, `read_inputs` and
  `finish`, and nothing else (ADR-028).
- **S-09.3** Given the monitor's build, when its sources are listed and any build script read, then the
  generator is not among them, and the monitor's check never reads the SDK record.
- **S-09.4** Given a contract that is not canonical, when the generator runs, then it refuses it.
- **S-09.5** Given a contract and an SDK, when `generate.py --check` runs, then it succeeds only if
  the SDK is byte for byte the one the contract generates (ADR-018).

### S-10 The audit report states Python ADR-008's limit

As an auditor, I want a contract task's report to say what its record does not prove, so that I do
not read a pass as proof the script ran as its source reads.

- **S-10.1** Given a passing record of the new version, when it is audited, then the report carries
  Python ADR-008's statement, verbatim (`ADR-008-findings-adopt-micropython-monty-is-not-ready.md:115-118`),
  the whole bold sentence, both clauses: "the record proves what the runtime computed from these inputs under this contract; it does not prove the script ran as its source reads, since hostile data may, through the runtime or its limits, have chosen the labels asked, the notification's body or the result, within the contract's grants." The audit does not try to tell a
  runtime guest from a native one (ADR-009; context §2 G).
- **S-10.2** Given a passing record of version 1 to 3 (the committed ones; none of version 4 has a golden report), when it is audited, then the report is
  unchanged and every existing golden report still holds (context §2 G, §3).
- **S-10.3** Given the golden report for a contract task, when it is produced, then its record
  passes audit under the `test-trust` stand-in log and provider in a test build only, and a release
  `hyalos-verify` refuses that record: inconclusive (exit 2) before the pin, refused (exit 1) after
  it (ADR-009, ADR-025).

## Rules and examples

### S-01 Long guest loops run on KVM

Rule: a guest's outcome on KVM does not depend on how many loop iterations it runs.

| ID | Given | When it runs on KVM | Then |
|---|---|---|---|
| S-01.1.e1 | a one-loop guest, 10,000 iterations | with the default image | ends as on the process backend |
| S-01.1.e2 | a one-loop guest, 1,000,000 iterations | with the default image | ends as on the process backend, or `OutOfFuel` on both if fuel runs out first; never `KernelFault` |

Rule: MicroPython's committed records replay byte-identically on both backends.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.2.e1 | the triage case on the issue fixture | run on each backend | transcripts equal each other and the committed record; replay passes on both |
| S-01.2.e2 | the committed record with one hex digit of its first answer's `question_sha256` changed to a different hex digit, so it still parses | replayed on each backend | replay ends `ReplayDiverged` on both (`hyalos-os/audit/src/replay.rs:51-65`) |

Rule: the fix changes no existing record.

| ID | Given | When replayed on the rebuilt image | Then |
|---|---|---|---|
| S-01.3.e1 | the pinned profile 2 transcripts (`spike_tests.rs` `PROFILE_2_BASE`) | on KVM | hash and `fuel_used` unchanged |
| S-01.3.e2 | the pinned profile 1 transcripts (`ring_tests.rs:15` `BASE`, seed and hash only, no fuel) | on KVM | hash unchanged |

Rule: the evidence reflects the fix.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.4.e1 | `expected.tsv` after the fix | read | every MicroPython case, hostile ones included, lists `both`; each that ends with a result has a committed record; a refused case gives the same refusal on both backends |
| S-01.4.e2 | a KVM host where the triage case ends `KernelFault` | `run.sh` runs | it fails |

### S-02 Inputs are length-framed under a new interface and record version

Rule: a framed record reaches the guest exactly as written.

| ID | Given a framed record | When the guest reads it | Then |
|---|---|---|---|
| S-02.1.e1 | a script of several lines | `read` | every byte, each `\n` included |
| S-02.1.e2 | a record of length 0 | `read` | a record of length 0, not dropped |
| S-02.1.e3 | bytes that are not UTF-8 | `read` | the same bytes |

Rule: a malformed framed file never starts a run.

| ID | Given | When the monitor reads it | Then |
|---|---|---|---|
| S-02.2.e1 | a stated length past the end of the file | on either backend | `BadInputs` before the guest loads |
| S-02.2.e2 | stray bytes after the last record | on either backend | `BadInputs` before the guest loads |

Rule: the new record version names its interface.

| ID | Given | When the record is written or read | Then |
|---|---|---|---|
| S-02.3.e1 | a run under `hyalos.abi/2` | written | new record version, naming `hyalos.abi/2` |
| S-02.3.e2 | a record of the new version with the interface name removed | read | refused |

Rule: the old interface section is never edited.

| ID | Given | When the conformance suite runs | Then |
|---|---|---|---|
| S-02.4.e1 | `ABI.md` with an `abi/2` section added | as committed | both pinned digests hold; the abi/2 digest half is proven in Slice 11, when that digest is pinned |
| S-02.4.e2 | one word of the `abi/1` section changed | as committed | the suite fails |

Rule: every older record keeps its meaning.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.5.e1 | each committed record, versions 1 to 3 (no version 4 record is committed under `hyalos-os/evidence/out/`; the spike's version 4 transcripts under `spikes/438/evidence/` hold no presentations and no golden report, and replay as before in the spike runs) | read, audited and replayed | passes as before |
| S-02.5.e2 | a version 4 record relabelled as the new version | read | refused |
| S-02.5.e3 | each committed real record with presentations, versions 2 and 3 (no version 4 record is committed under `hyalos-os/evidence/out/`; the spike's version 4 transcripts under `spikes/438/evidence/` hold no presentations and no golden report, and replay as before in the spike runs), after the template change | audited | passes with its golden report unchanged |
| S-02.5.e4 | a version 4 record holding an `abi` field | read | refused |

Rule: a version 4 run takes only `hyalos.abi/1` inputs, and version 5 waits for its pin.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.6.e1 | a version 4 run, a framed record holding a newline | the run starts | `BadInputs` |
| S-02.6.e2 | a version 4 run, single-line records | the run starts | it runs as today |
| S-02.7.e1 | a default build before the pin, a version 5 record | read | refused as inconclusive (exit 2) |
| S-02.7.e2 | the pin commit, the same record | read | accepted |

### S-03 The contract, SDK and script travel as inputs under the real profile

Rule: a contract task runs under the profile new runs use.

| ID | Given | When it runs | Then |
|---|---|---|---|
| S-03.1.e1 | contract, SDK, multi-line script and data | on each backend | the record names `hyalos.profile/2`; each input is one whole record |
| S-03.1.e2 | the same task naming `hyalos.spike/1` in a default build | on either backend | the profile lookup gives `unknown profile hyalos.spike/1`, and the run ends `ReplayDiverged` before any backend starts (`hyalos-os/audit/src/replay.rs:429`) |

Rule: the task identity covers every input byte.

| ID | Given a record | When one byte changes in | Then |
|---|---|---|---|
| S-03.2.e1 | as written | nothing | `read_transcript` accepts it |
| S-03.2.e2 | its task, before it runs | nothing; `task --version 5` is run on it in a `test-trust` build before the pin (`task` by default after it) | `task` prints the guest hash beside the `task_v5` identity (ADR-018) |
| S-03.3.e1 | as written | the script record | `read_transcript` refuses it |
| S-03.3.e2 | as written | the contract record | `read_transcript` refuses it |
| S-03.3.e3 | as written | a data record | `read_transcript` refuses it |

Rule: a re-run with fixed inputs gives the same record.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.4.e1 | the same inputs and seed | run twice | records byte-identical |
| S-03.4.e2 | the same inputs, a different seed | run twice | records may differ only in what the stand-in model answered and what follows from it |

### S-04 The monitor enforces the contract

Rule: a call passes only if the contract grants it; the reason is the existing one.

| ID | Given a contract that grants | When the guest calls | Then, on both backends |
|---|---|---|---|
| S-04.1.e1 | schema A, no other | `infer` on schema B | `UnknownSchema`; the source saw no question |
| S-04.1.e2 | schema A | `infer` on schema A | answered and recorded |
| S-04.2.e1 | no effect | `effect` of kind 0 | `UnknownEffect`; the source saw no effect |
| S-04.2.e2 | effect kind 0 | `effect` of kind 0 once | recorded |

Rule: a run without a valid contract never starts.

| ID | Given the contract record | When the run starts | Then, on both backends |
|---|---|---|---|
| S-04.3.e1 | absent | either backend | `BadContract` before the guest loads |
| S-04.3.e2 | its fields reordered | either backend | `BadContract` before the guest loads |
| S-04.3.e3 | with an unknown field | either backend | `BadContract` before the guest loads |
| S-04.3.e4 | the digest guest's contract, as committed | either backend | the run starts |
| S-04.3.e5 | the version 5 digest guest, record 0 a contract other than the digest contract | either backend | no digest |

Rule: a result outside `returns` is refused.

| ID | Given `returns` | When the guest returns | Then |
|---|---|---|---|
| S-04.4.e1 | allows the result | an allowed result | recorded |
| S-04.4.e3 | a size limit, after a granted effect | one byte more at `return` | `ResultLimit` at the `return` call; the effect already sent is not recorded |
| S-04.4.e2 | a size limit | one byte more than it | `ResultLimit`; no result recorded |

Rule: no default build knows a spike profile.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.5.e1 | a default build | a record naming `hyalos.spike/2` is read | the profile lookup gives `unknown profile hyalos.spike/2`, and its replay ends `ReplayDiverged` before any backend starts (`hyalos-os/audit/src/replay.rs:429`) |
| S-04.5.e2 | a default build | a record naming `hyalos.profile/2` is read | read as before |

Rule: replay enforces the record's own contract.

| ID | Given a contract record | When replayed | Then |
|---|---|---|---|
| S-04.6.e1 | as written | on both backends | passes |
| S-04.6.e2 | its contract edited to drop effect 0, task recomputed | on both backends | fails `UnknownEffect` |

### S-05 What a task accepts cannot exhaust the heap

Rule: inputs outside `accepts` never reach the guest.

| ID | Given `accepts` | When the data is | Then, on both backends |
|---|---|---|---|
| S-05.1.e1 | at most N records | N records | the run starts |
| S-05.1.e4 | `code` 2 | a code record of 16,385 bytes | `InputLimit` before the guest loads |
| S-05.1.e2 | at most N records | N + 1 records | `InputLimit` before the guest loads |
| S-05.1.e3 | at most B bytes per record | one record of B + 1 bytes | `InputLimit` before the guest loads |
| S-05.1.e5 | `code` 2 | fewer than `1 + code` records: the contract and one code record only | `InputLimit` before the guest loads |

Rule: anything `accepts` allows runs to its result under the stub model; a live run may still be refused by the attest tool's send limit (design.md §7, R15).

| ID | Given the triage contract | When the task runs on | Then, on both backends |
|---|---|---|---|
| S-05.2.e1 | its `accepts` | the largest input it allows | a result and its notification |
| S-05.2.e2 | its `accepts` | the spike's `heap` hostile case with each record cut to `record_bytes` | a result and its notification, never `MemoryError` |
| S-05.2.e3 | its `accepts`, every answer forced to each choice in turn, on the seed-0 input (40 records) | the seed-0 input; the largest accepted input is covered by `largest.record`, S-05.2.e1's row | a result and its notification each time |
| S-05.2.e4 | its `accepts` | an input holding one data record that is not UTF-8 | the SDK skips that record before asking; a result and its notification without it (ADR-018) |

Rule: every `accepts` limit carries its source.

| ID | Given | When it is read | Then |
|---|---|---|---|
| S-05.3.e1 | the triage contract's limits | in the evidence | each names its search, margin and command |
| S-05.3.e2 | a limit with no measurement | in review | it does not meet S-05.3 |

### S-06 Questions take closed answer types

Rule: the guest only ever receives a value of the schema's declared type.

| ID | Given a schema declared as | When the model answers | Then the guest receives |
|---|---|---|---|
| S-06.1.e1 | `bool` | yes | the SDK gives `True`; the guest's raw `infer` call returns the `i32` 1 |
| S-06.1.e2 | integer from 1 to 5 | 5 | the SDK gives 5; the raw call returns the `i32` 5 |
| S-06.1.e3 | choice of `a`, `b`, `c` | `b` | the SDK gives the string `b`; the raw call returns the `i32` 1, `b`'s index |

Rule: a contract declares only well-formed closed types.

| ID | Given a contract declaring | When it is read | Then |
|---|---|---|---|
| S-06.2.e1 | a free-text answer type | before the guest loads | refused |
| S-06.2.e2 | an integer from 5 to 1 | before the guest loads | refused |
| S-06.2.e3 | a choice list `a`, `a` | before the guest loads | refused |
| S-06.2.e4 | an empty choice list | before the guest loads | refused |
| S-06.2.e5 | an integer from 0 to 2^31 | before the guest loads | `BadContract` |

Rule: an out-of-type reply is refused live and fails replay.

| ID | Given a schema declared as | When | Then |
|---|---|---|---|
| S-06.3.e1 | integer from 1 to 5 | the live reply is 6 | `ModelRefused`; the guest sees nothing |
| S-06.3.e2 | choice of `a`, `b` | the live reply is `"d"` (`standin --answer '"d"'`) | `ModelRefused` |
| S-06.4.e1 | `bool` | a recorded answer is 2 | replay fails |
| S-06.4.e2 | `bool` | a recorded answer is `true`, recorded as label 1 | replay passes |

Rule: a passing record holds only closed-type requests.

| ID | Given a version 5 record whose presentation holds | When audited | Then |
|---|---|---|---|
| S-06.5.e1 | the closed-type request for its declared choice list | once | passes |
| S-06.5.e2 | a request whose answer is free text, sent by the test-only `free-text-ask` tool, which sends a file's bytes in place of `body(…)`, and signed blind by an unmodified notary keyed with the committed test key `golden/v5/notary.key` (copied into a fresh scratch `--dir` as `signing.key`, mode 0600) (ADR-028) | by a `test-trust` verifier on stand-in trust, given `golden/v5/notary.pub` as the trusted notary | refused (exit 1) by the template rule, with its message, not as an untrusted notary (ADR-023, ADR-025) |
| S-06.5.e3 | today's four-label request | once | refused |
| S-06.5.e4 | a request whose prompt differs from the contract's | once | refused |

Rule: a notary of another release and a run never open a session (the release is `RELEASE`).

| ID | Given | When the run opens its first session | Then |
|---|---|---|---|
| S-06.6.e1 | a notary and a run of the same version | once | the session opens |
| S-06.6.e2 | an old notary and a new-version run | once | refused; no ticket spent |

Rule: the release names the templates.

| ID | Given | When the test runs | Then |
|---|---|---|---|
| S-06.7.e1 | the templates and `RELEASE` as built | once | passes |
| S-06.7.e2 | one template byte changed, `RELEASE` not | once | fails |

### S-07 A per-task question limit

Rule: a run asks at most its contract's limit, counted per run.

| ID | Given limit | When the guest asks | Then, on both backends |
|---|---|---|---|
| S-07.1.e1 | 2 | a third question | `QuestionLimit`; the source saw two questions |
| S-07.1.e2 | 0, with a schema in `asks` | its first question | refused; the source saw none |
| S-07.2.e1 | 2 | two questions | both answered and recorded |

Rule: a record past its limit cannot pass.

| ID | Given a record with limit 2 | When | Then |
|---|---|---|---|
| S-07.3.e1 | holding 2 answers | replayed and audited | passes |
| S-07.3.e2 | holding 3 answers, task recomputed | replayed or audited | fails |

Rule: every contract declares a limit within the ceiling.

| ID | Given a contract | When it is read | Then |
|---|---|---|---|
| S-07.4.e1 | declaring a limit of 200 (the provisional ceiling; the fixed `QUESTION_CEILING` after Slice 11) | before the guest loads | the run starts |
| S-07.4.e2 | declaring no limit | before the guest loads | `BadContract` |
| S-07.4.e3 | declaring 201, one over 200 (the provisional ceiling; the fixed `QUESTION_CEILING` after Slice 11) | before the guest loads | `BadContract` |

Rule: a task that asks nothing spends no ticket, and is not audited.

| ID | Given a contract task | When it runs live with a notary | Then |
|---|---|---|---|
| S-07.5.e1 | that asks no question | to its result | no session opens, no ticket is spent, and the audit refuses the record |
| S-07.5.e2 | that asks one question | to its result | one ticket is spent at its first session |

### S-08 The question-limit ceiling is measured (#411)

Rule: every figure the ceiling rests on is measured against the real provider and states its path.

| ID | Given | When measured | Then |
|---|---|---|---|
| S-08.1.e1 | the largest input the triage contract accepts | against the real provider | call time and bytes per attestation measured against the real provider, recorded with command and path; concurrency taken from the notary's code and busy test, not a live two-session run (ADR-034, ADR-036) |
| S-08.1.e2 | a figure taken from the stand-in model | as the measurement | it does not meet S-08.1 |

Rule: the ceiling is recorded with its evidence.

| ID | Given | When | Then |
|---|---|---|---|
| S-08.2.e1 | the measurement | the ceiling is set | an ADR names the value and the figures |
| S-08.2.e2 | a ceiling with no ADR | in review | it does not meet S-08.2 |

Rule: a run that asks its full limit can finish.

| ID | Given limit L and the measured call time | When a live run asks | Then |
|---|---|---|---|
| S-08.3.e1 | L = 200 (the provisional ceiling; the fixed `QUESTION_CEILING` after Slice 11) | L questions at the measured time | it ends before its deadline |
| S-08.3.e2 | a stand-in delay of 1.4 × the measured time per call; L = 200 (the provisional ceiling; the fixed `QUESTION_CEILING` after Slice 11) for the deadline formula, and a contract with a limit of 5 for the run | L questions | the deadline for L = 200 covers 200 such calls; a run under the limit of 5 asks 5 and still ends before its deadline (ADR-024) |
| S-08.3.e3 | L = 1, 40 inputs | one question | the deadline is today's formula, 1,460 s |

### S-09 A contract yields its SDK

Rule: generation is a pure function of the contract's canonical bytes.

| ID | Given | When generated | Then |
|---|---|---|---|
| S-09.1.e1 | the triage contract | twice | identical bytes |
| S-09.4.e1 | the triage contract with its fields reordered | once | refused |

Rule: a customer can check an SDK against its contract.

| ID | Given | When `--check` runs | Then |
|---|---|---|---|
| S-09.5.e1 | the triage contract and its committed SDK | once | succeeds |
| S-09.5.e2 | the same SDK with one choice name remapped | once | fails |

Rule: the SDK exposes exactly the grants.

| ID | Given a contract that grants | When the SDK is generated | Then |
|---|---|---|---|
| S-09.2.e1 | one choice schema and effect 0 | once | one typed ask, one notify, `read_inputs` and `finish`, nothing else |
| S-09.2.e2 | one `bool` schema and no effect | once | one typed ask, no notify member, `read_inputs` and `finish` |

Rule: the monitor depends on neither the generator nor the SDK.

| ID | Given | When | Then |
|---|---|---|---|
| S-09.3.e1 | the monitor's build | its sources are listed and any build script read | no generator |
| S-09.3.e2 | a hand-written SDK that calls an ungranted schema through the raw import | the task runs | the monitor refuses it `UnknownSchema` |

### S-10 The audit report states Python ADR-008's limit

Rule: every new-version report carries the limit, and no older report changes.

| ID | Given a passing record | When audited | Then the report |
|---|---|---|---|
| S-10.1.e1 | of the MicroPython triage task, new version | once | carries the limit, pinned by a golden report |
| S-10.1.e2 | of the digest guest, new version | once | carries the limit |
| S-10.2.e1 | each committed record, versions 1 to 3 (`hyalos-os/evidence/out/*/record.json`; no version 4 record with a golden report is committed) | once | equals its existing golden report and carries no Python line |

Rule: the golden record passes only under test trust.

| ID | Given the golden contract-task record | When audited | Then |
|---|---|---|---|
| S-10.3.e1 | in a `test-trust` build | once | passes with the golden report |
| S-10.3.e2 | by a build without `test-trust` | once | before the pin, refused as inconclusive (exit 2); after it, refused (exit 1); either way before any network call (ADR-025) |
| S-10.3.e3 | a record answered through `standin --answer`, by a build without `test-trust` | once | before the pin, refused as inconclusive (exit 2); after it, refused (exit 1); either way before any network call (ADR-025) |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| Kernel stack overflow returns | a guest loop on a rebuilt image | the run ends as on the process backend | the same outcome on both backends; CI fails if not | S-01.1 |
| The fix changes old records | a dispatch change alters fuel | the slice stops; no profile's replay changes | pinned-transcript tests fail | S-01.3 |
| Truncated framed inputs | a stated length past the file's end | `BadInputs` before the guest loads | a refusal naming the inputs | S-02.2 |
| Interface section edited | a change to `abi/1`'s text | the conformance suite fails | a digest mismatch | S-02.4 |
| Swapped script | one byte of the script changed after authorisation | `read_transcript` refuses the record | a task mismatch | S-03.3 |
| Missing or malformed contract | input record absent or not canonical | `BadContract` before the guest loads | a contract refusal | S-04.3 |
| Ungranted call | the guest bypasses the SDK | `UnknownSchema` or `UnknownEffect` on both backends | the refusal; nothing reached the source | S-04.1, S-04.2 |
| Spike profile in a release | a record names `hyalos.spike/1` | refused as an unknown profile | an unknown-profile refusal | S-04.5 |
| Heap exhaustion by data | an input past `accepts` | `InputLimit` before the guest loads | an input-limit refusal | S-05.1 |
| Heap exhaustion within `accepts` | a limit set too high | the evidence run ends `MemoryError` and the slice fails | no notification; the case fails | S-05.2 |
| Out-of-type answer | the model replies outside the declared type | `ModelRefused`; the guest sees nothing | a refused run | S-06.3 |
| Free text attested | a request with a free-text answer reaches the notary | the notary signs it blind; the audit refuses the record | a refused audit | S-06.5 |
| Effect before refusal | a run refused after a granted effect was performed | the effect is not undone and no record is written | a refused run and a sent notification | S-04.4 |
| Non-UTF-8 data | a data record that is not UTF-8 | the SDK skips it before asking | a result without it | S-05.2 |
| Re-rolled questions | a run asks past its limit | `QuestionLimit` before the question leaves the monitor | a limit refusal | S-07.1 |
| Forged extra answers | a record holds more answers than its limit | replay and audit fail | an audit failure | S-07.3 |
| A full-limit run times out | the deadline is shorter than L calls at the measured time | the run ends before asking all it may | a deadline refusal; S-08.3 fails | S-08.3 |
| Over-trust of a Python pass | a report without Python ADR-008's limit | the golden report fails | a report diff | S-10.1 |
| Provider late | a live call's session passes the tool's deadline | today's reason, unchanged: `NotaryDeadline`, from the tool's word `deadline` (`hyalos-os/attest/src/main.rs:474`, `:536`; `hyalos-os/monitor/src/live.rs:105`), or `ModelUnavailable` if the tool itself outlives its 35-second limit (`live.rs:22`, `:155-163`); no passing record is written | a refused run | S-06.3 |
| Escaped question too large | a question within `MAX_QUESTION` whose JSON-escaped request passes `MAX_SENT` − 256 = 7,936 bytes | the attest tool refuses it before the session opens (`hyalos-os/attest/src/main.rs:209-212`); the error maps to `ProviderRefused`, and no ticket is spent (design.md §7, R15) | a refused run | S-06.3 |
| Calendar unreachable | the OpenTimestamps calendar (`hyalos-os/monitor/src/ledger.rs:27`) does not answer when `live` appends the intent or result | today's reason, unchanged: `LedgerFailed`; no passing record is written | a refused run | S-08.1 |
| Clock skew past `MAX_WAIT` | the operator's clock is more than `MAX_WAIT`, 10 s, behind the log's signed time (`hyalos-os/monitor/src/live.rs:29`, `:91-92`) | today's reason, unchanged: `LedgerFailed`, before the session opens | a refused run | S-08.1 |
| Unusable notary key file | `notary public --dir` finds a `signing.key` that is open to group or others, a link, not a regular file, or the wrong length | the key reader's fixed message on standard error, naming no key bytes; nothing on standard output; no key made; exit 2 (ADR-036) | a setup fault the customer fixes | S-08.1 |
| Provider unreachable or erroring | the provider cannot be reached, or replies with no answer the tool can read | today's reason, unchanged: `ProviderRefused`, from the tool's word `provider` (`attest/src/main.rs:470-479`, `:549-552`; `live.rs:108`); no passing record is written | a refused run | S-06.3 |
| A reply over `MAX_RECV` | the provider's reply passes the session's 16 KiB receive limit (`hyalos-os/attest/src/lib.rs:27`) | today's reason, unchanged: the tool's session fails and `word_for` names it (`attest/src/main.rs:470-479`), `ProviderRefused`, or `NotaryDeadline` if the deadline passed first (`live.rs:105`, `:108`); the notary refuses to sign past its limits (`hyalos-os/attest/src/bin/notary.rs:908`) | a refused run | S-06.3 |
| A run past its deadline | the whole live run outlasts `live_deadline` | today's reason, unchanged: `Timeout` (`hyalos-os/monitor/src/runner.rs:282`, `:344-351`) | a refused run | S-08.3 |
| Notary busy | a session arrives while one is open | refused at session open, nothing spent (`notary.rs:743`, `:1328-1332`); today's reason, unchanged: the closed connection is the tool's word `unreachable` (`attest/src/lib.rs:813-848`, `attest/src/main.rs:216`), so `NotaryUnreachable` (`live.rs:104`) | a refused session | S-06.6 |
| Notary of another release | the notary's `RELEASE` differs from the tool's | refused at session open, before the canary and `spend` (`notary.rs:745-754`); today's reason, unchanged: `ReleaseMismatch` (`live.rs:107`) | a refused session | S-06.6 |
| Canary held after the template change | the provider rejects the new template when the canary sends it. The pass is not tied to `RELEASE` (`hyalos-os/attest/src/lib.rs:395-410`), so a fresh pass from before the upgrade still counts. That stale pass, for up to 24 hours, is accepted (ADR-026) | sessions open as usual until the old pass expires, 24 hours after it (`CANARY_WINDOW_SECONDS`, `lib.rs:395`), or until a `canary check` records the rejection, since a held state is no pass (`lib.rs:402-410`); then every session is refused at open, before any ticket is spent (`notary.rs:764-768`), with today's reason, unchanged: `CanaryHeld` (`live.rs:111`). `hyalos-os/CHANGES.md`, created by Slice 6, advises re-running `canary check` after upgrading, so the canary sends the new template at once (ADR-023, ADR-026) | a refused session once the old pass expires | S-06.6 |
| The public log fails | the run's commitment or outcome cannot be written to the public chain | today's reason, unchanged: `LedgerFailed` (`hyalos-os/monitor/src/ledger.rs:88`) | a refused run | S-08.1 |
| The comment fails | `gh issue comment` fails or prints no comment link | today's reason, unchanged: `EffectFailed` (`live.rs:256-288`); a run refused after the comment was posted leaves it posted | a refused run | S-04.4, S-08.1 |
| Contract without a limit | `asks` with no declared limit, or one above the ceiling | `BadContract` before the guest loads | a contract refusal | S-07.4 |
