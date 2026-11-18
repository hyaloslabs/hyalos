# ADR-0108: The job state machine's TLA+ spec and its model check

**Status:** Proposed. Supersedes [ADR-0105](0105-the-required-check-gathers-ci-s-split-jobs.md)'s list of the jobs `build, lint, test` gathers.

## Context

S8.2 (#51). [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) decides that a TLA+ spec covers the job state machine, and that its invariants are each also a property checked on every simulated run, "so the spec and the code cannot drift apart unnoticed". It names four invariants: a task with an open hold has no running job; a merge to a protected branch happens only after an approval bound to the pull request's head commit; an expired manifest never authorizes work; the projections equal a replay of the log.

The simulated world ([ADR-0036](0036-the-simulated-world-and-the-log-scan.md), [ADR-0104](0104-fault-schedules-swept-in-ci.md)) already checked the first and the last at the end of a run, and checked that a run whose manifest expired ended `deadline_exceeded`. It never merged, so the second was never checked at all.

## Decision

**The spec is `spec/Job.tla`, checked under `spec/Job.cfg`.** It models tasks of two kinds (an actor's, and a merge's), each task's latest job, its open hold, the deadline in that job's manifest, the operator's merge approval, the pull request's head on the forge, what the forge merged, every step the broker admitted, and the log. Every action writes the events it records and updates the projection itself, and replaying the log is written apart from the actions, as the engine's replay is apart from its writes. The model has two tasks, two commits, a clock that stops at 2 and logs of at most 6 events, and tasks are symmetric.

**Its invariants are ADR-0013's, under names the code uses too:**

| Spec invariant | ADR | On every simulated run (`hyalos_world::spec`) |
| --- | --- | --- |
| `HeldTaskRunsNoJob` | 0009 | After every event in the log, no task with an open hold has a running job. |
| `MergeOnlyAtApprovedHead` | 0004, 0012, 0071 | Every merge the forge made was of the commit a `merge.approved` for that pull request named, while it was still the head, by a job on that approval's task. |
| `ExpiredManifestAuthorizesNothing` | 0004 | The broker admitted no step after its manifest's deadline. At the deadline itself the manifest is in date, as the broker's admission says. |
| `ProjectionIsReplay` | 0003 | The engine's state equals a replay of the log. |

The spec also checks `TypeOK` and `EndedTaskIsQuiet` (a completed or cancelled task has no open hold and no running job), which the engine's own `invariant_violations` checks on every run.

**Recording a hold and stopping the job are one transition.** In the spec, a gate's unknown answer writes `gate.answered`, `job.ended` (held) and `hold.opened` in one step, as the engine writes them in one `submit_all`, with the job ended first. So no state of the spec, and no prefix of a simulated log, has an open hold and a running job. The simulation checks this after every event, not only at the end of the run.

**Every invariant is shown to catch the mistake it is for.** The constant `Mutant` names one deliberate mistake: `split_hold` opens the hold a step before it stops the job, `stale_merge` merges whatever the head is, `late_work` admits a step after the deadline, and `unlogged_release` releases a hold without writing it to the log. `.github/scripts/check-spec.sh` model-checks the spec with `Mutant = "none"` and requires no error, then with each mutant and requires TLC to report the one invariant it must break. The simulation's tests do the same for its properties: each breaks one on purpose and requires the check to name it. A test reads `Job.tla` and `Job.cfg` and requires each of the code's four names to be defined and checked there, so renaming one side fails.

**The simulated run merges.** A job that succeeds with a pull request is followed, as `hyalos merge` runs it, by a merge task: the operator's signed `merge.approved` of the head they reviewed, one job in the broker with a manifest that carries the approval and asks for `merge` alone, and the fake forge asked to merge that commit. Half the time a push lands after the review, so the forge refuses with `merge.head_moved`. One merge in eight expires before it is asked for. The merge draws from a stream of its own, `merge`, so the job before it draws exactly what it did before. A sweep fails if no merge ended `Merged`, `HeadMoved` or `Expired`, so the merge invariant is never checked on nothing. The fake forge's commit ids are now 40 hexadecimal digits, as `CommitSha` requires, made from the same draws as before.

**CI checks the spec on every pull request.** A `job spec` job in `ci.yml` downloads the TLA+ tools at a pinned release (1.7.4) against a pinned SHA-256 and runs the script on the runner's Java. `build, lint, test` gathers it with `lint`, `test` and `simulation`, so it is required without an edit to the ruleset. This supersedes ADR-0105's list of the jobs it gathers; its rules for passing are unchanged. The tools are never part of the binary.

## Consequences

A change to the job state machine now has two places to change: the code, which the simulation checks, and the spec, which TLC checks. They share their invariants by name, and a test fails when the names drift. They do not share their transitions: the spec is a model of the engine, not generated from it, so a transition the code gains and the spec lacks is caught only when it breaks an invariant in the simulation.

The model is small: two tasks, two commits, three instants and six events. TLC explores every state within those bounds, but a bug that needs a longer history is outside them. Raising the bounds costs CI time.

The spec models one pull request and one job at a time per task. Several pull requests, policy, and budgets are left to the simulation and the engine's tests.
