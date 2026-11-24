# hyalOS: a kernel and monitor ready for independent review

| | |
|---|---|
| **Status** | executing |
| **Type** | Feature |
| **Size** | L — 12 slices |
| **Modifiers** | security, public-api, infra, integration |
| **Issues** | #457–#468 (Slices 1–12) |
| **Owner** | retroboot |
| **Opened** | 2026-11-23 |

## Brief

This is a child pack of [`hyalos-kernel`](../hyalos-kernel/README.md). Its boundary comes from the parent's [`pack-briefs.md`](../hyalos-kernel/pack-briefs.md) and the rows owned by `core` in [`coverage.md`](../hyalos-kernel/coverage.md).

**Problem.** hyalOS's kernel and monitor are still a spike. The guest interface is not fixed, and the monitor is not confined. The monitor gives the machine the host's full supported CPUID list (`spikes/task-kernel/kernel/src/kvm.rs:263`) and runs the kernel in ring 0, so a taken-over kernel can read the time counter and reach hardware randomness. Setting `CR4.TSD` alone does not stop this (measured, `context.md` §2). The security claim also rests mostly on code we did not write, `wasmi` and TLSNotary at `v0.1.0-beta.1`, and no review scope says whether either is covered (#412).

**Outcome.** The kernel and monitor can be handed to an independent reviewer: a stable guest interface and profile that capsules can build on, a guest with no unintended clock or randomness, a confined monitor, and a review scope that names every third-party component the security claim depends on, each marked covered or excluded with its reason.

**Scope.** In:

- A fixed guest interface, consuming the execution profile the verifier pack defines (`hyalos-kernel-verifier` ADR-007).
- The clock and hardware randomness paths in a taken-over kernel.
- Monitor confinement and hardening, including the lint and format debt the verifier pack leaves in the moved code.
- The new workspace's line budget and its rules for `unsafe` (parent ADR-001).
- The review scope for `tlsn` and `wasmi`, and how an upstream advisory reaches us (#412).
- The policy for leaving TLSNotary's beta pin.
- A verified package for the independent security review of hyalOS (ADR-006). Performing the review is tracked in a separate issue, and #63 still covers v1's trusted core.

Out: the task contract, connectors, any new guest call, Python guests (#438), and joining v1's engine.

**Depends on.** The verifier pack's Slice 1 (#443) moves `spikes/task-kernel` to `hyalos-os/`. This pack's slices cite the moved paths, and its final cold read runs after that move merges. Its execution profile comes from the verifier pack's Slice 2 (#444).

## Route

Confirmed by the developer on 2026-11-23.

- **Feature:** it adds guarantees to a kernel and monitor that already exist.
- **L:** the interface, clock and randomness, confinement, budget and `unsafe` rules, lint debt, third-party scope, pin policy and the review each need their own pull request.
- **security, public-api, infra, integration:** it hardens a trust boundary, fixes the guest interface capsules build on, changes CI and how the monitor is confined, and depends on upstream TLSNotary and `wasmi` advisories.

## Read order

1. This file, including **How to execute this pack** below
2. [`prd.md`](prd.md): problem, goals, non-goals, stories and acceptance criteria
3. [`context.md`](context.md): what exists today, with file citations, conventions and glossary
4. [`decisions/`](decisions/): the decisions and the options rejected
5. [`design.md`](design.md): architecture, contracts, test strategy, risks and modifier sections
6. [`plan.md`](plan.md): the slices, their order and acceptance criteria per slice
7. `progress/`: one file per slice once execution starts
8. [`cold-read.md`](cold-read.md): results of the cold-read review of this pack

## How to execute this pack

These rules are the same in every pack. They live here so an executor needs nothing but this
folder and the repository.

1. **Work one slice at a time.** A slice is one issue and one PR. Its issue body carries a line
   `Plan: docs/plans/hyalos-kernel-core · Slice: N`.
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

- Who performs the independent review. This does not block the pack (ADR-006).
- Whether `RDSEED` stays reachable on bare-metal Intel and on AMD. It was measured only on one nested Intel host, and the host probe (S-03.3) is how to find out (ADR-002).

## Retro

_Written by the final slice. Guidance: compare what the cold read predicted with what really
diverged, using the deviations in every progress file._

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
