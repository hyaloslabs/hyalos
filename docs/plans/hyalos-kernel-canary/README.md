# hyalOS: notice when the provider stops refusing a forged body

| | |
|---|---|
| **Status** | published |
| **Type** | Feature |
| **Size** | M — 4 slices |
| **Modifiers** | security, integration, public-api |
| **Issues** | #427–#430 (Slices 1–4) |
| **Owner** | retroboot |
| **Opened** | 2026-11-22 |

## Brief

**Problem (#409).** A request's 106 hidden bytes could hold a forged body. We treat that forgery as refused because the provider answered that shape with a 400 on 2026-11-21 (notary pack, ADR-011). That is the provider's behaviour, which can change. ADR-011 requires re-running `spikes/task-kernel/attest/probes/hidden_header.py` before relying on it, but that step is manual and nothing notices a changed answer.

**Outcome.** A changed provider response stops runs from being reported as attested, a test shows it, and the record says when and by whom the check last passed.

**Scope.** The check runs on the customer's side with the customer's own provider credential, because the operator is the party it guards against and controls the hidden bytes of any request it sends. The notary never holds that credential. Out of scope, as #409 states: other forged shapes, the hidden API-key case (the fifth limit, notary pack ADR-012, stays in every report), and the zero-knowledge proof that the hidden bytes hold no line break, which stays the lasting fix.

## Route

- **Feature:** a new customer-side check, with a new signed fact in the record and a new refusal.
- **M:** it touches the probe, the notary, the signed attestation and the audit, in a few independently shippable slices.
- **security, integration, public-api:** it guards a security claim, depends on a real provider's behaviour, and changes what the record and report state.

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
   `Plan: docs/plans/hyalos-kernel-canary · Slice: N`.
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
