# hyalOS: the task contract

| | |
|---|---|
| **Status** | published |
| **Type** | Feature |
| **Size** | L — 12 slices |
| **Modifiers** | security, performance, public-api |
| **Issues** | #508–#519 (Slices 1–12) |
| **Owner** | retroboot |
| **Opened** | 2026-11-25 |

## Brief

This is a child pack of [`../hyalos-kernel/README.md`](../hyalos-kernel/README.md), planned from its brief in [`../hyalos-kernel/pack-briefs.md`](../hyalos-kernel/pack-briefs.md) (`hyalos-kernel-capsules`).

The Python spike enforced a draft contract under spike profiles only. This pack turns it into the real contract for a hyalOS task: what a task may ask, touch and return, its limits, and the most questions it may ask. Tasks run on MicroPython, the runtime the spike's findings adopted ([`../hyalos-kernel-python/decisions/ADR-008-findings-adopt-micropython-monty-is-not-ready.md`](../hyalos-kernel-python/decisions/ADR-008-findings-adopt-micropython-monty-is-not-ready.md)).

The outcome is a contract the monitor enforces on KVM and as a process, with byte-identical records, and an audit report that states Python ADR-008's limit on hostile inputs.

In scope:

- The kernel image's stack overflow on long guest loops (#503) is fixed first, with a committed MicroPython record that replays byte-identical on both backends.
- Input records become length-framed, under a new guest-interface version (`hyalos.abi/2`) and a new record version. The contract, SDK and script then travel as inputs under the real profile, and the task identity covers every byte.
- The monitor's contract check replaces the spike's.
- Questions take closed answer types: `bool`, a bounded integer, and a choice from a declared list. The model never returns free text.
- A per-task question limit is declared in the contract and enforced by the monitor. Its ceiling is set from measurements against the real provider (#411): call time, bytes per attestation and the notary's concurrency, on the largest input the task accepts.
- This pack owns the SDK generator, kept outside the monitor.

Out of scope: real connectors, any chain a model assembles at run time, and work that spans tasks.

## Ruled out

- **A looser authorisation for new inputs.** No job needs it. One authorisation per run, naming the exact task and its inputs, already serves recurring jobs and keeps the re-roll defence the record pack built (record ADR-002).
- **Signed chains as their own construct.** A later step's inputs are an earlier step's output, so a chain of tasks cannot be authorised in advance under per-run authorisation. Multi-step work is one task's script, under one authorisation and one record. Work that spans tasks is v1's workflow under parent ADR-003, for the engine pack.
- **A signature on the contract itself.** The customer's authorisation of the task identity, which covers the contract, already binds it. Every other signer conflicts with an accepted decision: an operator-only signature (the operator would vouch for its own task, which the customer's authorisation exists to rule out), the engine's per-job manifest key, or a customer key (record ADR-003).
- **Free-text model answers.** A closed answer cannot carry an instruction into the next step or into an effect's body, and model text has no way out of its tainted type (ADR-0047, ADR-0059). Its capacity is still bounded, not zero: up to 32 bits per integer answer, so about 800 bytes over 200 questions (ADR-018).
- **New float or memory profiles.** MicroPython needs neither, and every verifier must keep supporting a profile for good. Only Monty, which is not adopted, needed floats and 5 MiB.
- **The contract for native guests first, Python later.** It would design the script and runtime rules without the one runtime that needs them.
- **Type checking before signing.** Only Monty had it, and the monitor's check is the boundary either way (Python ADR-008).

## Key insights

- With fixed inputs and a deterministic run, a re-run gives the same record. A task that asks no question spends no ticket from the notary. Its record has no attested answer and no ticket, so today's audit refuses it, and auditing such a task is out of scope (answered at the acceptance checkpoint on 2026-11-25; ADR-007).
- Framing and typed answers both change the guest interface and the record, so they share one version bump instead of two.
- MicroPython's heap is never collected, so memory bounds what the inputs may hold. `accepts` must state input limits derived from the heap, or a valid-looking input can suppress the one effect with a `MemoryError`.
- The question limit is counted per run, which fits with a ticket being spent at a run's first session (record ADR-004). The notary pack's Retro applies: when a count is the defence, ask at which moment the attacker has already seen what they want.
- The contract gets no key of its own. Two existing checks bind it: the customer's authorisation constrains the operator, and v1's policy and approval constrain the agent (parent ADR-003).

## Route

- **Feature:** a new contract, closed answer types and a question limit, behind stories with acceptance criteria.
- **L:** the #503 fix, framing under new interface and record versions, the contract check, typed answers, the question limit, the generator, the attestation-cost measurement and the audit report's limit.
- **security, performance, public-api:** it changes the guest interface, the record and the refusal rules; the #411 measurement sets the default limit; `hyalos.abi/2` and the new record version are read by every verifier.

## Read order

1. This file, including **How to execute this pack** below
2. [`prd.md`](prd.md): problem, goals, non-goals, stories and acceptance criteria
3. [`context.md`](context.md): what exists today, with file citations, conventions and glossary
4. [`decisions/`](decisions/): the decisions and the options rejected
5. [`design.md`](design.md): architecture, contracts and modifier sections
6. [`plan.md`](plan.md): the slices, their order and acceptance criteria per slice
7. [`cold-read.md`](cold-read.md): what a fresh reader could not answer, and how each gap was closed
8. `progress/`: one file per slice once execution starts

## How to execute this pack

These rules are the same in every pack. They live here so an executor needs nothing but this
folder and the repository.

1. **Work one slice at a time.** A slice is one issue and one PR. Its issue body carries a line
   `Plan: docs/plans/hyalos-kernel-capsules · Slice: N`.
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
5. **Stop conditions are hard stops.** When one triggers, stop, leave the branch intact, write the
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

- **Line budget.** Answered by spec on 2026-11-25: about 955 lines added and 410 removed, a net of about +545 against the 2,653 left (design.md, Line budget estimate). Each slice still runs `cargo xtask lines`.
- **A second job.** Answered by spec on 2026-11-25: the field list holds a support-mailbox job as it stands; only a new effect kind, from the connectors pack, is needed (design.md, What a second job would need).
- **The question-limit ceiling.** Every contract declares its limit, within a ceiling that is 200, provisional, until the #411 measurement at the largest input a task accepts replaces it (ADR-007, ADR-010). Nothing has been measured at the spike's 200-issue scale, where 200 attested calls would take about 6 minutes at 1.9 s a call with both sides in one process, or about 25 minutes at the one two-machine figure, 7.42 s.
- **An adversarial read of the design before slicing.** Done by spec on 2026-11-25: 14 findings, 4 blocking. All were applied with the developer's approval as ADR-015 to ADR-018 and revisions to the prd and design.
- **A stale citation in the brief.** `pack-briefs.md` cites the newline split at `kernel/src/main.rs:441`, which no longer exists. The split is at `hyalos-os/monitor/src/main.rs:202`.

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
