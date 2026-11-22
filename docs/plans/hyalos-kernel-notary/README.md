# hyalOS: an independent notary

| | |
|---|---|
| **Status** | published |
| **Type** | Feature |
| **Size** | M — 6 slices |
| **Modifiers** | security, integration, performance |
| **Issues** | #386–#391 (Slices 1–6) |
| **Owner** | retroboot |
| **Opened** | 2026-11-21 |

## Brief

First child of [hyalOS](../hyalos-kernel/README.md). Discovery was skipped because the parent states the problem, the outcome and the scope.

**Problem.** The spike attests every model answer with TLSNotary, but the notary runs inside the operator's own process under a key on the operator's machine (`spikes/task-kernel/attest/src/main.rs`). Whoever holds that key can sign an attestation for an answer the model never gave, so the attestation convinces nobody but the operator. Three more gaps sit beside it:

- **An attestation is not tied to its run.** The operator can ask the same question several times and keep the answer they like, or reuse an old attestation for the same question in a later run.
- **The audit trusts the public log's answers.** It does not check the log's inclusion proofs or each link's signature itself, and it derives the public key from the operator's private key file (`spikes/task-kernel/kernel/src/ledger.rs`).
- **The timestamp is requested and never verified.**

**Who runs the notary.** The developer decided on 2026-11-21: the first version is run by the customer, as their own service on their own machine, under a key the operator never holds.

**Outcome.** A third party who trusts neither the operator nor us can take a run's record and confirm, offline apart from the public log, that each model answer in it came from the provider's server in a session a notary outside the operator's control took part in, for this run and no other. An operator who holds every key on their own machine cannot produce a record that passes.

**Scope.** In: a notary that runs as its own service under a key the operator never holds, the operator's side talking to it over a network, binding each attestation to its run's intent, an audit that takes only public keys and verifies the public log's proofs itself, and a statement of exactly what a passing audit does and does not prove. Out: the record's long-term format and a packaged verifier (`hyalos-kernel-record`), hiding the question from the verifier, a bond on a public chain, and more than one model provider.

## Route

Confirmed by the developer on 2026-11-21.

- **Feature:** a spike exists, and this adds a separate notary service, run binding and a trust-free audit to it.
- **M — 6 slices:** binding to the run, the notary as its own service, the operator's side over a network, an audit with public keys and the log's own proofs, forgery tests with every operator key in hand, and the notary's cost report.
- **security:** the notary is the remaining point of trust in a run's record.
- **integration:** it depends on the model provider's TLS, the TLSNotary protocol and a public transparency log.
- **performance:** an attested call cost 1.9 s and about 650 MB in one process, and a network sits between the two sides now.

## Read order

1. This file, including **How to execute this pack** below
2. The parent, `docs/plans/hyalos-kernel/README.md`
3. [`prd.md`](prd.md): problem, goals, non-goals, stories and acceptance criteria
4. [`context.md`](context.md): what exists today, with file citations, conventions and glossary
5. [`decisions/`](decisions/): the decisions and the options rejected
6. [`design.md`](design.md): architecture, contracts and modifier sections
7. [`plan.md`](plan.md): the slices, their order and acceptance criteria per slice
8. `progress/`: one file per slice once execution starts
9. [`cold-read.md`](cold-read.md): results of the cold-read review of this pack

## How to execute this pack

These rules are the same in every pack. They live here so an executor needs nothing but this
folder and the repository.

1. **Work one slice at a time.** A slice is one issue and one PR. Its issue body carries a line
   `Plan: docs/plans/hyalos-kernel-notary · Slice: N`.
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

- **The spike is on `main`.** Settled on 2026-11-21: `spikes/task-kernel` landed in commit `1ccbe47`, identical to the tree this pack's citations were checked against.
- **What one attestation costs the notary.** The parent records a public notary service as a separate idea, priced per attestation. This pack should measure the notary's own CPU time, memory and bytes per attestation, so that idea has a number under it.
- **How to bind an attestation to a run.** One candidate is to carry the run's intent hash and the question's position inside the attested request itself, in a field the provider accepts and ignores. Untested against the provider. A weaker fallback is to require the attestation's time to fall between the run's two links in the public log.
- **Collusion.** A notary that colludes with the operator defeats this. Whether one notary of the customer's choosing is enough, or a threshold is needed, depends on who the customer is.
- **Cost.** The spike measured 1.9 s and about 650 MB peak per attested call with the notary in the same process. Over a network it will be slower, and the protocol needs a low-latency link between operator and notary.
- **What the notary learns.** It never sees plaintext, but it learns that a session with the provider happened, when, and how many bytes moved.
- **Availability.** If the notary is down, the fail-closed rule says the task does not run. This pack settles it: the run is refused (prd G4, S-02.2). A mode that runs unattested would be a new decision.

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
