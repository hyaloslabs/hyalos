# hyalOS: a kernel for bounded tasks you can prove

| | |
|---|---|
| **Status** | routed |
| **Type** | Greenfield |
| **Size** | XL — 6 child packs |
| **Modifiers** | security, integration, data, public-api, performance, infra |
| **Issues** | not yet published |
| **Owner** | retroboot |
| **Opened** | 2026-11-21 |

## Brief

This pack replaces the gated wall, an earlier plan whose seven child packs are no longer planned and whose drafts were not kept in this repository. The gated wall tried to contain a general-purpose agent. This plan stops giving the model a computer at all.

**Problem.** Every sandbox product still hands the agent a general-purpose machine, then tries to wall it in. The incidents that matter were not walls breaking: a stolen provider key, and a file the agent wrote that a trusted tool outside the box later ran. Containing the driving agent also made Hyalos hard to install, because the wall had to sit between two OS users. And a record the operator alone signs proves nothing to anyone else, because a compromised or dishonest signer can issue any record it likes.

**Outcome.** Hyalos becomes a runtime for signed, bounded tasks. The unit of work is a task, and a model is one possible engine inside it:

```text
receive    ->  connect         ->  work              ->  notify
no model       credential          model if needed       credential
               no model            no credential         no model
               fixed query         typed answer          fixed template
```

A task runs on hyalOS, a kernel that boots directly on KVM with no Linux inside and offers four calls and nothing else:

```text
read(input)       sealed, typed
infer(question)   approved model, typed answer
effect(action)    one signed action, performed by the host
return(result)    typed

Does not exist: files, network, processes, threads, clock, randomness
```

The rule is: **scripts act, decision engines decide, and a model only answers a typed question.** No task ever holds a credential, so injected text reaches nothing it can abuse. The part that can act has no judgment to manipulate.

**What a run proves, and to whom.** Each run leaves a record that a stranger can check without trusting the operator:

| Claim | How it is shown |
|---|---|
| The task did only this with these inputs and answers | Replay: the run is deterministic, so anyone re-executes it and must get identical bytes |
| No second, contradictory record exists for this run | A public, append-only chain of salted commitments, one before the run and one after |
| The model provider's server really sent this answer to this question | A notary's attestation of the TLS session |
| Nothing else was touched | The machine has no way to touch anything else |

**The product.** Work you can prove, between organisations: a task that runs in the buyer's or the seller's environment and hands back evidence the other party verifies alone. Agents become optional callers and authors of tasks. They never hold access.

**Scope.** In: the child packs below. Out: containing a general-purpose agent, judging whether an answer is correct, building and test jobs (they need a real computer and stay on the existing microVM substrate), a connector catalogue to rival workflow tools, a token, and any task data on a public ledger.

**What stays from the gated wall.** Hyalos vouches for conduct, never competence. Humans build Hyalos. The operator's approval rule is unchanged: what the operator created and signed runs, a task from an outsider waits, and an outsider is anyone who could not already do this themselves (ADR-0103).

## Ruled out

- **A separate OS user for Hyalos, with a second account for the agent.** It makes Hyalos something only an engineer can install, and the operator's own account was root-equivalent through the `docker` group anyway.
- **Putting the driving agent in a box.** It is another mousetrap: the wall only covers agents started through Hyalos, and the danger is what is inside the box and what gets trusted when it comes out.
- **A watcher or auditor service that reviews outputs and learns from history.** It reviews after authority is granted. In a replay of 223 merged pull requests, learning from approval counts waved through 107 exceptions.
- **A better VM for agents.** Several vendors sell one and a major one was donated to a foundation this month. They all still give the agent a computer because agents expect one.
- **Competing on connectors and ease of use.** Workflow products own that. Credential proxies, isolated steps, self-hosting and audit logs are no longer gaps either.
- **Receipts signed only by the operator.** One party both runs the work and keeps the evidence. The spike forged a consistent record that replayed perfectly.
- **Waiting for model providers to sign their responses.** Research found none that does.
- **Hardware enclaves as the base of trust.** Attestation keys have been extracted from current chips with cheap equipment.
- **A JIT compiler in the kernel.** Recent escapes in WebAssembly runtimes were compiler bugs. Tasks mostly wait on APIs, so an interpreter is fast enough.
- **A role prompt as containment.** Telling a model it is a toaster steers it. What contains it is that the task has no tools and one typed output.
- **A model assembling chains of tasks at run time.** That is improvisation again. A model may draft a chain, and only a chain the operator signed runs.
- **Writing a general-purpose operating system.** The kernel exists to run one bounded task and then cease to exist.

## Key insights

- **A model does not need a computer.** It takes text in and gives text out. The danger comes from the shell and credentials placed around it.
- **Deleting is the design.** Firecracker won by removing what one workload did not need. A bounded task lets us remove files, network, processes, threads, the clock and randomness, and each removal buys a guarantee: no socket instruction exists, and a timing attack has no stopwatch.
- **AI removes the ecosystem barrier.** New operating systems die because no software runs on them. Here the smallest Claude model wrote a working task from the interface document alone, in 205 lines and 2,920 bytes, and it ran first time.
- **Replay proves conduct, not truth.** Two runs with different model answers both replayed perfectly. Replay shows the task handled its answers honestly. It cannot show the model gave them, which is why the commitment chain and the attestation exist.
- **Three layers close three different lies.** Replay catches a record altered after the fact. The public chain catches a second record, a hidden re-run and an unlogged record. The attestation catches an invented model answer.
- **The notary is the remaining trust.** In the spike the notary ran in the operator's own process, so the operator could still forge an attestation. Who runs the notary is a product decision as much as a technical one.
- **Inference moves from run time to build time.** A model drafts the lookup script once, the operator signs it, and it then runs with no model. "Hyalos learns" means it accumulates signed software, not that it trusts a pattern of past approvals.
- **A taken-over kernel still has only five requests.** The monitor outside treats the kernel as hostile and checks every request, and it only reads or writes the kernel's heap and stack. Hostile kernels written as raw machine code were refused.
- **The monitor is the new trusted part.** It holds the credential and performs the effect, so its size and confinement matter more than the kernel's.
- **Two gaps looked open in the market.** Proof of a negative ("touched these and nothing else"), which needs the enforcer and the record to be the same mechanism, and trust between organisations, where the other party asks what your automation did with their data.
- **A chain adds exactly one thing.** A bond that anyone can slash by showing two signed records for one task. Everything else a public transparency log already gives, and task data must never go on a permanent ledger.

**What the spike measured** (`spikes/task-kernel`, one x86-64 machine under WSL2, my own tests and not an independent audit):

| Check | Result |
|---|---|
| Same task as a process and on bare KVM | Records byte-identical, each replays under the other |
| Hostile tasks, hostile kernels, memory probes, tampered records | 51 of 51 refused |
| Random requests at the monitor | 3,000,000 with no crash |
| Time per run on KVM, 40 stand-in inferences | About 4.5 ms |
| Real run: KVM, real model, one real notification, public commitments | Passed its audit and replays offline |
| Forgeries against the commitment chain | 4 of 4 exposed |
| Attested model call | 1.9 s and about 650 MB peak per call |
| Forgeries against the attestation check | 7 of 7 refused, and 406 of 406 altered presentations |

## Child packs

Confirmed by the developer on 2026-11-21.

| Order | Pack | Covers | Depends on |
|---|---|---|---|
| 1 | `docs/plans/hyalos-kernel-notary` | A notary the operator does not control, each attestation bound to its run, and an audit that needs no trust in the operator | none |
| 2 | `docs/plans/hyalos-kernel-record` | The record's versioned format, a standalone verifier a customer runs, checking the public log's own proofs, and keeping task data private during verification | 1 |
| 3 | `docs/plans/hyalos-kernel-core` | The kernel and monitor from spike to something reviewable: a fixed interface, the hardening backlog, confinement of the monitor, and an independent review | none |
| 4 | `docs/plans/hyalos-kernel-capsules` | The signed task contract (accepts, returns, may touch, engine, limits), typed questions beyond one label, and chains the operator signed | 2, 3 |
| 5 | `docs/plans/hyalos-kernel-connectors` | Sources and channels held outside the kernel, one credential per task, and the outsider rule on what may run | 4 |
| 6 | `docs/plans/hyalos-kernel-engine` | Joining the existing engine, policy, signed log and broker, and what happens to the v1 coding workflow | 3, 4 |

### Parent-level pass (2026-11-22)

ADR-001, ADR-002, ADR-003 and the pack order are accepted by the developer. Where this differs from the table above, this section governs: the order is canary, verifier, core, a Python guest spike, the v1 boundary (ADR-003), capsules, connectors, engine integration.

- [`pack-briefs.md`](pack-briefs.md): a short brief for each unplanned pack and the order, adding `hyalos-kernel-verifier` (#434) and the Python guest spike.
- [`coverage.md`](coverage.md): every deferred item and open question, with an owner or an explicit outcome.
- [`ADR-001`](decisions/ADR-001-hyalos-is-its-own-workspace-in-this-repository.md): hyalOS is its own Cargo workspace in this repository.
- [`ADR-002`](decisions/ADR-002-keep-wasmi-and-name-the-execution-profile.md): keep the pinned `wasmi`, and make each record name its execution profile.
- [`ADR-003`](decisions/ADR-003-v1-is-the-control-plane-and-hyalos-runs-bounded-steps.md): v1 is the control plane, and hyalOS runs its bounded steps.

## Route

Confirmed by the developer on 2026-11-21.

- **Greenfield:** the kernel, monitor, record and verifier are new, and only a throwaway spike exists.
- **XL — 6 child packs:** the work spans proof, the record, the kernel, the task contract, connectors and the existing engine.
- **security, integration, data, public-api, performance, infra:** it is a trust boundary, it depends on outside services (a notary, a transparency log, model providers), it defines a stored record and a guest interface, start-up and attestation time matter, and it needs KVM.

## Read order

1. This file, including **How to execute this pack** below
2. `spikes/task-kernel/ABI.md`, the guest interface the spike used

## How to execute this pack

These rules are the same in every pack. They live here so an executor needs nothing but this
folder and the repository.

1. **Work one slice at a time.** A slice is one issue and one PR. Its issue body carries a line
   `Plan: docs/plans/hyalos-kernel · Slice: N`.
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

- **A public notary service, as a separate idea.** The developer's idea of 2026-11-21: offer attestation to any caller, not only hyalOS users, at about $10 per 1,000 attestations. The first notary is the customer's own, so this is not in any child pack yet. What it needs before it is a plan: the notary's measured cost per attestation (the first pack measures it), why a stranger would trust our key (a notary that colludes with its caller defeats the proof, and we cannot be the independent notary for work we run ourselves), what a notary is liable for, and who already sells this.
- **Whether anyone pays for this.** No buyer has been asked. Five conversations with vendors who sell automated work to regulated clients would answer it better than more analysis. This is not a pack.
- **Where the code lands.** `spikes/` is not part of the workspace or the trusted core. The kernel carries an interpreter of tens of thousands of lines we did not write, and core has a 30,000-line budget (ADR-0017). `hyalos-kernel-engine` must settle whether the kernel and monitor are core, a crate, or a separate repository.
- **Which interpreter, and what sits under the kernel.** The spike used `wasmi`. seL4 as a verified base under the kernel was suggested and not evaluated.
- **A clock inside a taken-over kernel.** A task has no clock, but code running in the kernel can read the CPU's time counter. The spike does not stop that.
- **How much real work fits.** One-shot tasks and short signed chains are proven for one issue digest. A second, non-GitHub job and a coding job are untested.
- **Replay and privacy.** Replay needs the inputs and an attestation shows the question in plain text. Private data needs selective disclosure or a zero-knowledge proof, and the cost of running the interpreter inside one is unknown.
- **The model's identity.** An attestation proves the provider's server sent the bytes. The model name in them is the provider's own claim.
- **Where this pack lands at publish.** Status goes to GitHub issues, requirements to a new PRD in `docs/prd/`, and decisions to `docs/adr/` with their features (ADR-0014). The pack itself is not committed to main.
- **TLSNotary's maturity.** The spike used `v0.1.0-beta.1` from its git tag.

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
