# hyalOS: a versioned record with a customer-set attempt budget

| | |
|---|---|
| **Status** | done |
| **Type** | Feature |
| **Size** | M — 6 slices |
| **Modifiers** | security, data, public-api |
| **Issues** | #413–#418 (Slices 1–6) |
| **Owner** | retroboot |
| **Opened** | 2026-11-22 |

## Brief

Second child of [hyalOS](../hyalos-kernel/README.md), after [the independent notary](../hyalos-kernel-notary/README.md). Discovery was skipped because #408 and #410 state the problem, the outcome and the scope.

**Problem.** The notary pack ties each answer to its run and position, but the operator still decides how many runs exist. They can run a task several times and hand over the record they like (#408). The audit reports how many runs the public log holds for the task, and that count can be avoided in two ways:

- **The operator can vary the task.** The task identity is a hash of the guest and its inputs (`task_of`, `spikes/task-kernel/kernel/src/main.rs:761`), so a slightly changed input is a new task with a count of one.
- **The count is per operator key.** S-04.5 of the notary pack counts runs under one key, so rehearsals logged under another key are not counted.

Three further gaps sit beside it:

- **The task identity is ambiguous.** `task_of` hashes the inputs joined with no framing, so the input lists `["ab", "c"]` and `["a", "bc"]` get the same identity (#408).
- **The record has no long-term format.** The spike's transcript and log links carry a version number, but nothing defines how a later verifier reads an older record or treats a field it does not know.
- **The "providers do not sign" assumption has no date and no place to change.** On 2026-11-02 Anthropic released FlashProve, a provable-inference research prototype. It is not an API feature, so the decision stands today, but the model's identity remains the provider's own claim and the record has nowhere to carry a provider-side proof if one arrives (#410).

**Outcome.** The customer, not the operator, sets how many attested attempts a job gets. The customer authorises the job and its attempt budget, the customer's notary issues one ticket per attempt and attests only sessions on its own tickets, and it signs how many tickets it issued. An operator cannot obtain more attested attempts than the customer authorised without the audit failing or reporting it. The record has a versioned format with framed inputs and a typed slot for a provider-side proof, and a verifier never treats a proof type it does not know as proof.

**Scope.** In: #408, #410 and the versioned record format both need. Out, and staying with the parent plan for a later record pack: a packaged standalone verifier, checking the Bitcoin timestamp (deferred here by the notary's ADR-005), and keeping task data private during verification. Also out: the provider-behaviour canary (#409), producing or verifying any real provider proof, and stopping an operator rehearsing a question directly with the provider, off the notary. The operator holds the provider credential, so tickets cap attested attempts only, and the audit report must state that as a limit.

**How the design is challenged.** The notary pack's retro found that a cold read checks a pack can be followed but does not attack its design. This pack plans an adversarial read of the design from the start, before it is sliced.

## Route

Confirmed by the developer on 2026-11-22.

- **Feature:** it adds a customer authorisation, notary tickets and a versioned format to the spike's existing notary and audit.
- **M — 6 slices:** the enforced format, the audit's replay, customer authorisation, notary tickets, the audit of the attempt with adversarial tests, and the provider-proof slot.
- **security, data, public-api:** the operator is the attacker, the record is stored and read long after its run, and customers and verifiers depend on its format.

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
   `Plan: docs/plans/hyalos-kernel-record · Slice: N`.
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

None.

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
| A legacy audit could pass ticketed attestations between Slices 4 and 5 (Q18), so S-01.6 moved to Slice 4 | The check held, but once the notary signs tickets a real ticketless presentation cannot be made. S-01.6.e2 and the version 1 tests use a wrapper around `verify` | 4 | Before a change ends an old output format, keep a real fixture of that format |
| Each slice's file list was complete | Slices 1, 4, 5 and 6 changed files the plan did not list. Review of Slice 4 found the two-machine probe broken by a new required setting | 1, 4, 5, 6 | When a slice changes a shared signature or adds a required setting, search for every caller, scripts included |
| Not predicted | `include!` cannot take `notary.rs`, which opens with inner doc comments, so `forging-notary` loads it as a module | 5 | Compile a proposed build mechanism during planning |
| Not predicted | `ask`'s own `verify` refuses a doubled or short ticket before writing it, so the audit's test needed another test hook | 5 | Trace a forged artifact through every check before the one under test |
| Not predicted | A Slice 4 test script could exit before the monitor wrote to it, which failed intermittently and surfaced in Slice 5 | 4, 5 | Repeat new process-spawning tests before merging, and make stand-ins read their input as the real tool does |
