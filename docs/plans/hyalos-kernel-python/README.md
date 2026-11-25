# hyalOS: a signed Python task with a contract-generated SDK

| | |
|---|---|
| **Status** | executing |
| **Type** | Feature |
| **Size** | M — 5 slices |
| **Modifiers** | security |
| **Issues** | #494–#498 (Slices 1–5) |
| **Owner** | retroboot |
| **Opened** | 2026-11-25 |

## Brief

This is the Python guest spike from the parent pack ([`../hyalos-kernel/README.md`](../hyalos-kernel/README.md),
section "Python guest spike" in [`../hyalos-kernel/pack-briefs.md`](../hyalos-kernel/pack-briefs.md)),
tracked as #438. It must finish before `hyalos-kernel-capsules` fixes the task contract, because
capsules adopts the guest runtime rules this spike supports and owns the SDK generator if the spike
succeeds. The parent's ADR-002 (keep `wasmi` and name the execution profile) and ADR-003 (v1 is the
control plane and hyalOS runs bounded steps) bound it.

**Problem.** Today a hyalOS task is a WebAssembly guest written against four calls, and the monitor
enforces only fixed limits: schema `0`, one effect, and size limits. Nothing lets a customer write a
task as a short Python script, and nothing ties what a task may ask or touch to a contract the
monitor checks. A guest whose SDK leaves out `notify` can still call `effect`.

**Outcome wanted.** An answer to one question: can a task be a signed Python script, run by an
interpreter compiled into the guest, on hyalOS's four calls, with a small SDK generated from its
contract? The answer names which runtime capsules should adopt, MicroPython or Monty, or states that
neither is ready, with measurements behind it.

**Scope (#438).**
1. A draft contract that grants one classification schema and one notification.
2. A Python SDK generated from it, deterministically. The script gets only the granted methods, and
   no client, key, model setting, URL or tool list. The SDK's bytes are bound into the task identity
   with the script.
3. A minimal contract check for `asks` and `may_touch` in the monitor, on both backends, so the
   monitor, not the SDK, refuses an ungranted call.
4. The issue-triage task rewritten in Python against the SDK, run under MicroPython and Monty as
   guests. MicroPython needs a custom port that imports only `hyalos`; Monty needs a new build
   target, floats, more than 4 MiB of memory and its `getrandom` use handled. Both need a
   multi-line script delivered as a separate field or framed input, because the input loader splits
   on newlines.
5. Guest size, cold start, run time and fuel measured under `wasmi`.

Any change to floats or the memory limit stays inside this spike; adopting one is a new execution
profile under ADR-002 and a decision for capsules.

**Out of scope.** CPython on WASI, Muru, the real provider SDKs, and replacing the microVM. Signing:
the spike binds the contract, SDK and script into the task identity (S-02.4, S-03), and signing
that identity is capsules' concern; the findings ADR says so (developer, cold-read round 4).

**Done when (#438).**
- The same triage script and generated SDK run unchanged under each runtime that builds, and each
  run replays byte-identically on both backends.
- A script that calls a method its contract omits fails, and no ungranted call reaches the host.
- A hostile guest that calls the raw `hyalos` imports directly is refused for any schema or effect
  its contract does not grant.
- Hostile inputs are tested against each runtime, and what they can steer inside the guest is
  recorded. A runtime bug can change a task's answer or its one effect, and replay would reproduce it.
- Changing the contract changes the SDK's bytes and the task identity.
- Monty's type checking is tried against an ungranted call, and the result is recorded. A failure
  here does not fail the spike.
- The findings name which runtime capsules should adopt, or state that neither is ready.

## Route

- **Feature:** #438 is a spike in purpose, but answering it builds a monitor contract check on both
  backends, an SDK generator, two interpreter ports and hostile tests: several reviewable PRs with
  acceptance to prove. The last slice writes the findings ADR, which is the spike's answer.
- **M:** about five or six slices, from the contract and generator through the two runtimes to the
  measurements, hostile inputs and findings.
- **security:** the pass criteria are refusals (an ungranted call, a raw-import guest) and hostile
  inputs steering a runtime, so the design needs a threat model. No performance modifier: the spike
  measures, with no numeric target to meet.

## Read order

1. This file, including **How to execute this pack** below
2. [`prd.md`](prd.md): problem, goals, non-goals, stories and acceptance criteria
3. [`context.md`](context.md): what exists today, with file citations, conventions and glossary
4. [`decisions/`](decisions/): the decisions and the options rejected
5. [`design.md`](design.md): architecture, contracts and modifier sections
6. [`plan.md`](plan.md): the slices, their order and acceptance criteria per slice
7. `progress/`: one file per slice once execution starts
8. [`cold-read.md`](cold-read.md): results of the cold-read review of this pack

## How to execute this pack

These rules are the same in every pack. They live here so an executor needs nothing but this
folder and the repository.

1. **Work one slice at a time.** A slice is one issue and one PR. Its issue body carries a line
   `Plan: docs/plans/hyalos-kernel-python · Slice: N`.
2. **Preflight before editing.** Confirm that every path and symbol `context.md` cites, and that
   your slice touches, still exists as described. Record anything that differs in your slice's
   progress file as a deviation. If the difference touches your slice's **Contracts**, stop.
3. **Mandatory beats guide.** In `plan.md`, every slice's **Outcome**, **Acceptance**,
   **Contracts**, **Stop conditions** and **Verify** are mandatory. **Pattern to follow**,
   **Suggested path** and **Files** are guidance. You may leave the suggested path if every
   mandatory item still holds. Record why in your progress file.
4. **Trace acceptance.** Every acceptance criterion ID in your slice (for example `S-04.2`), and
   every example row under it in the PRD's **Rules and examples** (for example `S-04.3.e1`), must be
   shown to hold. Use a test that names the ID or, where the repo has no test runner, a verification
   step recorded against the ID in your progress file. Follow the levels in the design's **Test
   strategy**.
5. **Stop conditions are hard stops.** The one exception is a stop condition `plan.md` marks with the words "a
   recorded result" (its **Recorded blockers** paragraph): that ends the slice `done`. When any other triggers, stop, leave the branch intact, write the
   blocking question into your progress file with status `blocked`, and report. Do not work around it.
6. **Do not silently edit the plan.** If the PRD, an ADR, `design.md` or another slice turns out to
   be wrong, record it as a deviation. If it changes a contract, that is a stop.
7. **Write your progress file** at `progress/slice-NN.md` (`NN` is the slice number, zero-padded)
   and commit it in the same PR as the slice. Give it these sections, in order:
   - **Header:** Status (`in-progress`, `blocked` or `done`), Issue, PR, Executor model if known,
     Date.
   - **Preflight:** each citation checked, and whether it holds.
   - **Acceptance:** each criterion ID and example ID, and how it was shown.
   - **Deviations:** a table of plan said / reality / why / contract affected (y/n).
   - **Decisions:** choices the plan left open.
   - **Discoveries:** what the next slice or the retro should know.
   - **Blocked on:** only when blocked; the question and the stop condition that triggered.

   If this README's Status is `published`, change it to `executing` in the same commit.
8. **Close the pack when you finish the last slice.** If every other slice's progress file says
   `done`, add a **Retro** section to this README in the same PR and set **Status** to `done`.

## Open questions

None for planning. (Where the contract check lives, since `spikes/task-kernel` is gone, is settled
by [ADR-001](decisions/ADR-001-the-spike-lives-in-hyalos-os-behind-a-feature.md).) Whether each
runtime builds, fits and runs deterministically is what the spike exists to answer: those are
`design.md`'s risks R1 to R4 and the PRD's assumptions, each with its recorded outcome in Slices 3
to 5.

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
