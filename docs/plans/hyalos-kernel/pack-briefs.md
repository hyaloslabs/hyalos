# hyalOS: briefs for the unplanned packs

**Status:** order and ADRs accepted by the developer (2026-11-22). These are boundaries, not plans. Each pack still goes through its own discovery, spec and slicing, and may change what is written here.

## Order

```text
canary (in progress)
  -> verifier
  -> core
  -> Python guest spike
  -> v1 boundary applied to the contract (ADR-003)
  -> capsules
  -> connectors
  -> engine integration
```

The verifier moves ahead of core because it needs no kernel work and delivers the promise that a stranger can check a run alone. Core may run alongside it once ADR-002's execution profile exists, because both depend on that one definition. The Python spike must finish before the capsule contract is fixed, and it may start earlier because it uses the existing spike.

v1 and hyalOS are converging, so ADR-003 sets the boundary between them before capsules. Only that decision moves earlier. The integration build stays last because it needs core and capsules.

## `hyalos-kernel-verifier` (#434)

- **Purpose:** a program a customer gets and runs alone to check a run's record.
- **In:** packaging the audit, proof that the customer's copy is genuine, every record version, and the report's stated limits.
- **Out:** running tasks, holding any key, and judging whether an answer is correct.
- **Needs:** ADR-001, ADR-002's profile, and canary's record version 3.
- **Provides:** the public way to read a record, which capsules must extend rather than bypass.
- **Known work:** the audit runs an external `openssl` for signature checks (`kernel/src/ledger.rs:35`) and a separate `attest` program for attestations (`kernel/src/main.rs:678`). A packaged verifier must carry or replace both.
- **Decided:** the Bitcoin timestamp and private verification are out of the first version. The log's signed time already serves the audit (notary ADR-005), and private verification is a research problem of its own.
- **Python guests:** replay needs the guest's exact bytes, so the verifier must obtain a runtime image by its hash. It treats that image as data to execute, and needs no Python support of its own.

## `hyalos-kernel-core`

- **Purpose:** turn the spike's kernel and monitor into something reviewable.
- **In:** a fixed guest interface, the execution profile, hardening, confinement of the monitor, its line budget and `unsafe` rules, and the independent review (#63, #412).
- **Out:** the task contract, connectors, and any new guest call.
- **Needs:** ADR-001 and ADR-002.
- **Provides:** a stable guest interface and profile for capsules and the verifier.
- **Known work:** the monitor gives the guest the host's full supported CPUID list (`kernel/src/kvm.rs:263`) and leaves `CR4.TSD` clear. A taken-over kernel can therefore read the time counter, and may be able to reach hardware randomness. The randomness path has not been tested. This is the parent's clock question, and it is wider than the parent states.
- **Decide in the pack:** which third-party components the review covers (#412), and the policy for leaving TLSNotary's beta pin.

## Python guest spike

- **Question:** can a task be a signed Python script, run by an interpreter compiled into the guest, on hyalOS's four calls?
- **Method:** generate a small Python SDK from a draft contract, write the issue-triage task against it, and run it under MicroPython and Monty as guests.
- **Measure:** guest size, cold start, run time and fuel under `wasmi`, byte-identical replay on both backends, and hostile scripts and hostile inputs.
- **Out:** CPython on WASI, because it needs a new deterministic WASI layer in the monitor, Muru, which is a server for long-running web apps and not a capsule runtime, and the real provider SDKs, which need libraries neither runtime can load.

### The generated SDK

The script gets only the methods its contract grants. It has no client, key, model setting, URL or tool list. For a contract that grants one classification schema and one notification:

```python
from hyalos import task

issues = task.read_inputs()
labels = [task.ask.classify(issue.title) for issue in issues]
task.notify.team(render(labels))
task.finish(summary(labels))
```

- **The SDK is not the boundary.** It makes the allowed calls easy and removes the others from the script's view. The monitor is what refuses a call the contract does not grant.
- **The monitor needs a contract first.** Today it enforces only fixed limits: schema `0`, one effect, and size limits (`spikes/task-kernel/kernel/src/main.rs:26`). A guest that omits `notify` from its SDK can still call `effect`. The spike adds a minimal contract check for `asks` and `may_touch` to the spike's monitor, on both backends.
- **Generation is deterministic.** The same contract gives the same SDK bytes.
- **The SDK is part of what is signed and replayed.** Its bytes are bound into the task identity with the script, so a changed SDK is a different task.
- **Each runtime exposes it its own way.** Monty through host functions, and MicroPython through a native module over the `hyalos` imports. The Python surface stays the same.

**The spike passes when:**

1. The same triage script and generated SDK run unchanged under each runtime that builds, and each run replays byte-identically.
2. A script that calls a method its contract omits fails, and no ungranted call reaches the host.
3. A hostile guest that calls the raw `hyalos` imports directly, bypassing the SDK, is refused by the monitor for any schema or effect its contract does not grant.
4. Changing the contract changes the SDK's bytes and the task identity.
5. Monty's type checking, if it can be used, rejects a script that calls an ungranted method before it is signed. A failure here is recorded and does not fail the spike.
6. The result names which runtime capsules should adopt, or states that neither is ready.

What the candidates need, from reading their source at Monty `v1.6.0` and MicroPython's current tree. Neither has been built for hyalOS yet:

| Need | MicroPython | Monty |
|---|---|---|
| A module importing only `hyalos` | Needs a custom port. Its WebAssembly port uses Emscripten | Needs a new target. Its worker builds for `wasm32-wasip1` |
| No floats | Possibly, with floats configured out | No. It would need a float-enabled profile |
| 4 MiB of memory | Likely | Unlikely. The `monty` crate alone is about 99,000 lines, before Ruff's parser |
| No host randomness | To be checked | Needs work. It depends on `getrandom`, though random seeds can be supplied |

**What a guest runtime does not remove.** A runtime bug cannot reach anything outside the guest, so it adds nothing to the trusted base for containment. It can still change what the task does. Hostile input that exploits the runtime could choose the task's answer, or the body of its one permitted effect, and replay would reproduce that faithfully. A record proves what the guest did, so the runtime stays in scope for the claim that the script ran as written.

The spike must therefore test hostile inputs as well as hostile scripts, and the audit report must state this limit for any Python task.

## `hyalos-kernel-capsules`

- **Purpose:** the signed contract that says what a task may do.
- **In:** the contract, typed questions beyond one label, signed chains, an explicit inference limit (#411), and the guest runtime rules the Python spike supports.
- **Out:** real connectors, and any chain a model assembles at run time.
- **Needs:** core's interface, the record format, the Python spike's result, and ADR-003's boundary with v1's manifest and policy.
- **Provides:** the contract that connectors and the engine enforce.
- **Conflict to settle:** a customer's authorisation names one exact task, including its inputs (record ADR-002). A contract meant to run on new inputs needs a looser authorisation under a new extension version, without reopening the re-roll attack that the attempt budget closed. Python makes the split concrete: the runtime and script stay fixed and signed, and only the data varies.
- **Interface limit:** the input loader splits files on newlines (`kernel/src/main.rs:441`), so a multi-line script cannot be one input today. The contract needs a separate script field or a framed input.
- **Decide in the pack:** whether to allow floats, and a higher per-task memory limit. Each is a new execution profile (ADR-002), so neither changes quietly.
- **Generated SDK:** if the spike succeeds, this pack owns the generator, and the monitor's contract enforcement replaces the spike's minimal check.

Draft shape, to expose conflicts early. It is not a contract:

| Field | Meaning | Exists today as |
|---|---|---|
| `guest` | Hash of the signed task or runtime image | `guest_sha256` |
| `script` | Hash of the signed script, for a runtime guest | Nothing |
| `profile` | Execution profile | Nothing (ADR-002) |
| `accepts` | Input types and limits | Untyped lines |
| `asks` | Question schemas and the inference limit | Schema `0` only, with no limit on the count |
| `may_touch` | The effects it may request | One notification |
| `returns` | Result type | Up to 4 KB, untyped |
| `limits` | Fuel, memory and sizes | Constants in the monitor |

## `hyalos-kernel-connectors`

- **Purpose:** reach real sources and channels without a task ever holding a credential.
- **In:** connectors held outside the kernel, one credential per task, and the outsider rule on what may run (ADR-0103).
- **Out:** a catalogue to rival workflow tools. Start with what a second real job needs.
- **Needs:** the capsule contract's `may_touch`.
- **Provides:** the host side of `read` and `effect` for real systems.
- **Decide in the pack:** which second, non-GitHub job proves the design. The parent lists this as untested.

## `hyalos-kernel-engine`

- **Purpose:** build the join between v1 and hyalOS that ADR-003 defines.
- **In:** the adapter that runs a task as a step, policy admission of a signed contract, the log event linking a record, credential handoff to the monitor, and the first joint use.
- **Out:** moving hyalOS into `core/` (ADR-001), and making hyalOS a `Substrate` driver. Building and test jobs stay on the microVM substrate.
- **Needs:** ADR-003, core and capsules.
- **Constraint:** the trusted core has 1,406 lines left, so this integration must stay small or pay for itself by removing code.
- **Decide in the pack:** whether v1's reader step is the first joint use, and how a credential reaches the TLS prover.
