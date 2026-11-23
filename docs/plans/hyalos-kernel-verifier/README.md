# hyalOS: a standalone verifier a customer can run alone

| | |
|---|---|
| **Status** | executing |
| **Type** | Feature |
| **Size** | L — 7 slices |
| **Modifiers** | security, data, public-api, integration, infra |
| **Issues** | #443–#449 (Slices 1–7) |
| **Owner** | retroboot |
| **Opened** | 2026-11-22 |

## Brief

Child of [hyalOS](../hyalos-kernel/README.md), and the "later record pack" that `hyalos-kernel-record` deferred to. It covers #434. Discovery was skipped because the issue, the parent's [`pack-briefs.md`](../hyalos-kernel/pack-briefs.md) and the parent's ADR-001 to ADR-003 state the problem, the outcome and the scope.

**Problem.** The parent promises a record that a stranger can check without trusting the operator. Today the check is the `audit` subcommand of the spike's kernel binary. A customer would have to build it from `spikes/task-kernel`, it runs an external `openssl` and a separate `attest` program, and it links the code that runs tasks. A record also does not name the interpreter settings its replay depends on, so an interpreter update could make an honest old record fail with no explanation.

**Outcome.** A customer gets a verifier without the operator's involvement, confirms that their copy is genuine, and checks a real run's record using only that verifier, public keys and the public log. A pass never claims more than the audit shows.

**Scope.** In:

- Moving hyalOS from `spikes/task-kernel` to the separate Cargo workspace `hyalos-os/` (parent ADR-001). No other pack owns this, and the verifier needs it first.
- One execution profile that both backends and the verifier build from, named by each new record (parent ADR-002).
- A verifier program that links no KVM, notary-service or operator-signing code.
- Reading every record version, refusing what it cannot read, and reporting an unknown provider proof as unchecked.
- A way for the customer to confirm that their copy of the verifier is genuine.

Out:

- Checking the Bitcoin timestamp, and keeping task data private during verification. Both are parked in the parent's [`coverage.md`](../hyalos-kernel/coverage.md).
- Other forged request shapes (#441) and the zero-knowledge proof of the hidden bytes (#442).
- The Python guest spike (#438), which starts after the code move.
- The monitor's hardening and independent review (`hyalos-kernel-core`), and joining v1's engine (`hyalos-kernel-engine`).

## Route

Confirmed by the developer on 2026-11-22, and re-routed from M to L on 2026-11-23.

- **Feature:** the audit exists in a spike, and this adds a program a customer can obtain and run alone.
- **L:** seven slices: the code move, the execution profile, the extracted verifier, the forgery fixtures and refusal corpus, signature and attestation checks without external programs, a genuine copy, and a real run verified with the docs published. The forgery work leaves the extraction slice because nine cold-read rounds kept finding blockers in that slice and the next one.
- **security:** the verifier is what a customer trusts instead of the operator, so a forged copy or a false pass defeats the product.
- **data:** a new record version names the execution profile, and every older version must still be read.
- **public-api:** customers depend on the verifier's command line, exit codes, report wording and record format.
- **integration:** it checks the public transparency log's proofs and TLSNotary presentations.
- **infra:** it adds a Cargo workspace, CI for it, and a release path for the verifier.

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
   `Plan: docs/plans/hyalos-kernel-verifier · Slice: N`.
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

- **Operator steps.** After Slice 6 merges, the operator runs the verifier release workflow's manual, non-publishing mode, so Slice 7 can prove S-05.1 to S-05.3. The first real `verifier-v*` tag is also the operator's.
- **Settled.** ADR-006 settles how a customer confirms a genuine copy. Separately, by `plan.md` 5.1, the verifier's check depends on `tlsn-attestation` and `tlsn-core` instead of the full `tlsn` crate, which a trial dependency resolution on 2026-11-23 put at 149 packages against `attest`'s 289. Slice 5 proves it by building the check.

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
