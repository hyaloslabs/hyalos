# hyalOS: a kernel and monitor ready for independent review

| | |
|---|---|
| **Status** | done |
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
- Which hosts trap `RDRAND` and `RDPID`. They ran on one CI run, whose CPU the log did not name (likely AMD), and faulted on a later one on Intel (Xeon 6975P-C, `kvm_intel`); the host probe (S-03.3) is how to find out (ADR-009).

## Retro

Written by Slice 12 on 2026-11-25, from `cold-read.md`'s five rounds and the deviations in all twelve progress files. Every slice is done. The pack ends at a ready review package (ADR-006). The review itself is #491, and #63 still covers v1's trusted core.

| Cold read predicted | What diverged | Slice | Lesson for the next pack |
|---|---|---|---|
| Round 1 Q-16, round 2 C-07, round 5 C9: only `RDSEED` is host-dependent, and the 64-bit probe's measurement governs | `RDRAND` and `RDPID` ran in ring 3 on one of CI's runners with their CPUID bits clear. AMD's SVM cannot trap them. The slice blocked, and ADR-009 made them host-dependent like `RDSEED`. Since then CI has measured an AMD EPYC 7763 with all three reachable, a Xeon Platinum 8573C with `RDSEED` reachable, and a Xeon 6975P-C with all blocked | 3, 4, 12 | Measure hardware behaviour on every runner type in the CI pool before writing it into acceptance criteria. One development host is one data point |
| Rounds 2 and 5 (Q-06, Q-09, Q6): the `live` guard is a few lines after `load_backend`, tested through the binary | `two_machines.sh` was a committed caller of `live` on a real repository without a kernel, and the canary tests run that script. The slice blocked until the developer chose to fix the script in it | 5 | Preflight should search for every script and test that calls a command, not only the code that defines it |
| Round 1 Q-02: "cold start" is boot to first call, within 20% | The metric held (about 2.0 ms against 2.1 ms), but the runner split made the whole start 3.8 ms to 7.0 ms (+85%), a cost the NFR's metric does not see | 6 | When a slice adds a process boundary, define the performance target over the whole start, or name the cost the target leaves out |
| Round 2 Q-15: the seccomp list may grow, each addition with a reason | The measured list differs from the design's in both directions: `recvfrom` and `sendto` instead of `read`, `close` and a `CLOCK_MONOTONIC` `clock_gettime` that a trace cannot see on a vDSO host, and six listed calls never made. The clock call was found by review reading `kvm::boot_and_run`, not by the `ptrace` trace: on a host whose clock source has a vDSO path it makes no system call, so a list measured by tracing alone would have killed the runner on a host without one | 7 | Write the allow-list's rule and restrictions in the design, and leave the list to measurement and code reading. A system-call trace cannot see vDSO calls, so read the code for every clock and CPU call too |
| Rounds 3 to 5 (Q6, Q14, K7, C5): the `unsafe` list starts with seven files | Eleven: `runner.rs`, and three verifier-pack tests that need `env::set_var` or `pre_exec` | 9, 10 | A list copied from a snapshot goes stale when another pack is merging into the same tree. Name the rule, and let the check hold the list |
| Rounds 3 and 4 (Q16, Q17, Q19): the scope lists the components ADR-005 and ADR-008 name, each with a dated audit search | An advisory (GHSA-qrr7-2g7f-r9rg) named the pinned `wasmi` 2.0.2 mid-slice. The slice blocked, and the developer moved to 2.0.3 with `hyalos.profile/2` (ADR-0134). Review also added rows for `ureq`, `bincode`, `wasmparser` and the trust anchors, and `not searched` for utility crates | 11 | Run the advisory check when the pack is planned and again before the scope slice. A pin the pack cites can move under it |
| Round 4 K1 and Q12: the hyalOS counter is a function of its own with `core/`'s rules | Review found that moving a `#[cfg(test)]` line drops code from the count, and that `unsafe /* */ {` passed a next-token match. Both were tightened. The same gap in `core/`'s counter is #487 | 10 | Plant each failure a check claims to catch before calling the check done. Review found every gap here by planting |
| Not predicted | Every slice's Verify needed CI's two `attest` builds first, because `attest/tests/notary.rs` runs the built programs. Slice 12's Verify, run exactly from a clean clone, shows the same | 1–12 | A cold read should run each Verify block from a clean clone, not only read it |
| Not predicted | KVM changes the CPUID list at run time: it sets leaf `1`'s `APIC` bit from the APIC base it enables by default, so with the allow-list alone the kernel saw `PAE` and `APIC`. The monitor now writes `IA32_APIC_BASE` to 0 before the start. KVM also answers leaves past the highest with leaf `1`'s filtered data, not zeros | 3 | Test what the guest sees from inside the machine, not the list the monitor sets |
| Not predicted | CodeQL raised `rust/cleartext-logging` on `hyalos-os/monitor/src/kvm.rs` twice in one slice: a unit test's assertion message printed a word of a made-up host's brand string, then a leaf number from the filtered CPUID list. Both messages became fixed text | 3 | Host identity is the data this pack hides, and a test message is output too. Write assertion messages as fixed text when a test handles host data |
| Not predicted | CI found what the development host could not: on CI's host the runner inherited two pipes the CI agent left open without close-on-exec, which `core_s_04_1_e1` caught. The runner now closes every inherited descriptor (`close_range`) before it opens KVM, and refuses where that fails | 6 | A process started by something we do not control inherits what it left open. Test confinement on CI's hosts, not only the development host, and close inherited descriptors rather than trust close-on-exec |
| Not predicted | The KVM deadline's `SIGALRM` did not reach the virtual CPU off the main thread, so a spinning kernel hung a test. Slice 6's runner fixed it by moving the deadline to the holder | 2, 6 | Hostile tests under a test harness run on other threads. Check that a signal-based mechanism is tested off the main thread |
| Round 3 Q13, round 3 C8, round 5 Q16: the commit under review is the pull request's merge-base | Held. CI's `lint` job needed `fetch-depth: 0` so the ancestor check can see that commit, and the check fails closed on a shallow clone | 12 | A check that reads history needs a checkout that holds it. Say so in the contract |
| Cold read and `context.md`: citations are found by symbol when lines move | Held. Line drift appeared in almost every preflight and never touched a contract | 1–12 | Keep citing by symbol and line together |
| Not predicted | `ledger::fixture_tests::local_4_2_generate` flaked in several slices on a timing-dependent message. It has no issue | 7, 8, 9 | File a flake when it is first seen, so later slices can cite it instead of rediscovering it |
