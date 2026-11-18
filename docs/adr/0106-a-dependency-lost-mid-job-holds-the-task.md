# ADR-0106: A dependency lost mid-job holds the task

**Status:** Proposed. Supersedes [ADR-0053](0053-an-interrupted-run-ends-its-job.md)'s `broker.stopped` row, and its `sandbox.failed` row for a sandbox the runtime lost.

## Context

#54 (S8.5). [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) asks for chaos scenarios that kill each dependency mid-job, and every scenario must end with the task held, never proceeding unguarded. A running job depends on four things:
- the broker and its channel;
- the sandbox;
- the model provider;
- the forge.

Until now, losing any of them ended the job `failed`:
- a broker that closed its channel ended it `broker.stopped`;
- a sandbox its runtime lost ended it `sandbox.failed`;
- a model provider still busy or unreachable after the relay's retries was refused to the harness as `provider.overloaded`;
- a forge that could not be reached, or answered 5xx, was refused to the harness.

A failed last job completes the task. So the run went on as if the failure were an answer, and nobody was asked. A job that stopped half-way may have pushed a branch, or may have been about to. Whether it did is exactly what is not known.

## Decision

**A lost dependency is the `dependency` gate answering `unknown`.** `Gate` gains `dependency`. This is schema version 22: `gate.answered` and `hold.opened` may name it, and no payload changes. Wire version 12 lets the broker's `gate` message carry it.

An unknown answer holds, as for any gate ([ADR-0020](0020-gate-answers-timeouts-and-holds.md)). In one write:
- the answer is recorded;
- the job ends `held` with the loss's code;
- a hold opens with stakes `irreversible` and reason `unknown`, so only a human releases it.

The task is not completed. The operator retries it or cancels it.

**Who reports a loss:**

| Dependency | Who sees it | Code |
|---|---|---|
| The broker or its channel | the engine, when the channel closes during the job, or is gone when the job would start its sandbox, or when a reader or planner step is asked | `broker.lost` |
| The sandbox | the engine, when the runtime reports the sandbox `lost` | `sandbox.lost` |
| The model provider | the broker, when the relay refuses `provider.overloaded` after its retries | `model.lost` |
| The forge | the broker, when a pull-request or git call is refused `transport.unreachable`, `transport.bad_response` or `forge.upstream_status` | `forge.lost` |

The broker sends its loss as a `gate` message with answer `unknown`, through the same reports as the other gates. The harness still gets its refusal. The engine holds the job when it reads the message, and stops the sandbox. When the engine sees the loss itself, it records the hold with `Origin::Engine`. A lost broker also stops the sandbox at once: with the channel gone, nothing the sandbox does can be reported or guarded any more.

**What is not a loss:**
- The operator's own interrupt. It ends the job `run.interrupted`, as before.
- A refusal the broker already reported. That refusal is the reason.
- A sandbox that exits non-zero, or that the engine stopped. These still end `sandbox.failed`.

Logs written before this ADR keep `broker.stopped` and read as before.

**The chaos suite** is `core/hyalos/tests/chaos.rs`. It runs the real engine, policy, broker and GitHub App code against the stand-in GitHub and a local-process sandbox. It kills, mid-job:
- the broker process, with `SIGKILL`;
- the sandbox's process, with `SIGKILL`;
- the model provider, which nothing answers;
- the forge, which the stand-in takes down.

Each scenario must end in time with:
- the job `held` with the loss's code;
- a `hold.opened` on the `dependency` gate;
- no `task.completed`;
- no push and no pull request.

The suite is in the `hyalos` crate, so it runs in the `test` part that `build, lint, test` gathers on every pull request ([ADR-0105](0105-the-required-check-gathers-ci-s-split-jobs.md)).

## Consequences

An outage of the provider or the forge now leaves held tasks behind rather than failed, completed ones. The operator sees them in the log and in the agent's `list_approvals`, and decides once the dependency is back: `hyalos approve <hold>` to run on, or `hyalos cancel <task>`. This is more work for the operator during an outage, and it is the point: the engine does not guess what a half-run job did.

A provider that stays busy through the relay's retries is treated as lost, not as a slow answer. A harness that would have waited longer is held instead.

The simulator's world already holds on a gate that is down ([ADR-0036](0036-deterministic-simulation-harness.md)). These scenarios check the same rule against the real processes.
