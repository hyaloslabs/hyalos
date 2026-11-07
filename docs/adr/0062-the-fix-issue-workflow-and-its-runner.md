# ADR-0062: The fix-issue workflow, and the runner that mints only from the check

**Status:** Accepted

## Context

S4.6 (#78) asks for three things:
- A shipped workflow (reader, planner, actor, open PR) installs and passes the role checks.
- A copy edited so tainted text reaches the actor's prompt fails to install.
- `hyalos run --issue-number` runs this workflow, and `DirectPrompt` and `ForgeIssue::direct_task` are deleted ([ADR-0059](0059-tainted-text-cannot-reach-an-actor-prompt-by-type.md)).

What exists:
- [ADR-0060](0060-the-workflow-document-and-the-check-at-install.md) defines the workflow document and its check, and derives each step's inputs (`Checked::inputs`). It leaves the runner that mints from them to this story.
- [ADR-0061](0061-reader-steps-run-in-the-broker.md) runs a reader as one model call in the broker, which answers typed values only. It leaves to this story how the engine chooses the protocol and model, and whether a planner uses the same call.
- `hyalos run` runs one actor job ([ADR-0050](0050-running-one-job-from-the-command-line.md)). With `--issue-number`, it puts the issue's text in the actor's prompt through `DirectPrompt` ([ADR-0052](0052-reading-the-jobs-issue-from-github.md)), the exception that ADR-0059 named and gave an end.
- The engine allows one running job per task ([ADR-0019](0019-typed-events-and-in-memory-projections.md)).
- [ADR-0005](0005-provenance-is-a-type.md) and [ADR-0016](0016-agent-runtime-and-broker-protocols.md) say a planner with no tools needs no sandbox. A planner that needs to read the code is an actor variant that returns its plan through `hyalos-submit` (S4.7, #84), which is not built.
- An actor reads raw text only through a gated handle, released by an approved decision (S4.4, #26). That is not built either.

## Decision

**The fix-issue workflow ships in the binary.** `core/hyalos/workflows/fix_issue.json` is compiled in (`workflow_run::FIX_ISSUE`). Every run parses it with `Workflow::parse`, the same check as at install, before anything is written. It has two inputs, `issue` (`forge_issue`) and `task` (`operator_text`), and three steps:

| Step | Role | Reads | Returns |
| --- | --- | --- | --- |
| `triage` | reader | `issue` | `kind` (bug, feature, docs, test, chore), `size` (small, medium, large), `clear`, `has_repro` |
| `plan` | planner | the four `triage` fields | `approach` (minimal_fix, fix_with_tests, docs_only, investigate), `add_test` |
| `fix` | actor | `task`, `triage.kind`, `triage.size`, `plan.approach`, `plan.add_test` | nothing; it pushes and opens a PR |

The check derives that all three run tainted. Only `triage` reads `ForgeAuthored`, and the other two read `Operator` and `ModelFromTainted`. Editing the actor's reads, or the planner's, to name `issue` fails the check with `workflow.raw_text_to_actor` or `workflow.raw_text_to_planner`.

**The runner runs only what the check passed** (`hyalos::workflow_run::Runnable`):
- The shapes it runs are readers and planners followed by exactly one actor, last (`workflow.not_runnable`). This holds until an actor can return values (S4.7).
- Every input a step reads must be supplied (`workflow.input_missing`). Every `forge_issue` input is the run's issue number, and every `operator_text` input is the run's task.

**One task, one job per step, in order.** Each step is a job of the run's task. Each job is ended before the next starts, as the engine requires. One broker serves the whole run, and every job ends by one run deadline (`--minutes`). A job that fails stops the run: no later step starts, and the run's summary is that job's.

**A reader or planner job runs in the broker.** Its `job.started` records the substrate `broker` and the attestation `none (attestation.no_sandbox)`, since nothing runs in a sandbox. Its manifest has:
- the step's role, and `tainted` as the check derived;
- the action `read` only if the step reads the issue, which gets it a read-only token of its own, revoked when the job ends;
- no branch, no pull request base, no hosts and no tools;
- the flow `job->model`;
- a budget of at most 32,000 tokens (`STEP_BUDGET_TOKENS`), within `--budget-tokens`.

It is minted with `Checked::inputs` for the step, so the step's provenance comes from the check, not from code written beside it.

The engine builds the `ReaderCall` from the step:
- its instructions;
- the issue number, if it reads the issue;
- the operator's task as context, if it reads it;
- each earlier value it reads, as context `step.field: value`.

It sends the call with `BrokerProcess::read`, waiting at most 180 s (`STEP_WAIT`), within the deadline. Then it sends `end_job`. A good answer is `job.ended` `succeeded` with `step.answered`. A refusal is `failed` with the broker's own code (`forge.no_such_issue`, `reader.unknown_field` and so on).

**A planner uses the reader's call.** `hyalos_broker::reader::run` accepts the planner role, with its own framing ("You plan from the values you are given"). A planner that names an issue is refused (`reader.planner_reads_issue`), so the broker holds the planner to typed values as the check does. A planner that needs to read the code is S4.7's sandboxed variant, and that decides its tools then. The relay's refusal of tools stays keyed to the reader role ([ADR-0037](0037-model-protocol-adapters-and-their-line-budget.md)). The broker builds a planner's request itself, with no tools, so the refusal is not needed for it.

**The step model follows the harness.** Codex and test commands use OpenAI, and Claude Code uses Anthropic. Both go through the same `provider.keys`. The model is `--step-model <name>`, or by default `gpt-5-mini` or `claude-haiku-5-5` (`DEFAULT_*_STEP_MODEL`). These are small models for a small JSON answer.

**Values come back typed, with provenance.** The engine turns what the broker returned, already checked by `values_fit`, into `workflow_run::Values`:
- a `Sourced<bool>`, a `Sourced<i64>`, or a `Sourced<Choice>` picked from the workflow's own options (`Choices::operator`);
- each marked `ModelFromTainted` when its step ran tainted.

A later step is given only the values it reads.

**The actor's prompt is an `ActorPrompt`** (`Runnable::actor_prompt`). It holds, in order:
- the engine's header (the repository);
- "This work is for issue #N" (the operator's number, as a typed value);
- the step's instructions;
- the task;
- each value the step reads, as `- step.field: value`;
- the engine's steps (clone, branch, push, `hyalos-pr`).

Nothing in it is text a model or the forge wrote, and `ActorPrompt` could not hold such text in any case. The runner checks that what the prompt took is within the `Checked::inputs` of the actor's step, or refuses (`workflow.inputs_mismatch`). It then mints the actor's manifest with those checked inputs.

**`--issue-number` runs the workflow, and the direct run is gone.** `DirectPrompt`, `DirectTask` and `ForgeIssue::direct_task` are deleted. The `<issue>` block and its `&lt;` escaping now exist only in `ReaderPrompt::issue`, which the broker builds. `--issue <text>` may now come with `--issue-number`, as the operator's description of the work (the workflow's `task`). Without it, the task is "Resolve issue #N of owner/name." `--issue` alone is still one actor job with no workflow, since its text is the operator's.

## Consequences

S4.6's acceptance holds:
- The shipped workflow installs, and the check derives its taint (`workflow_run` tests).
- An edited copy that sends the issue to the actor or the planner does not install.
- `--issue-number` runs triage, plan and fix as three jobs of one task, and `DirectPrompt` and `direct_task` no longer exist.

The tests:
- `core/hyalos/src/workflow_run.rs`: the shipped workflow, the edited copies, the shapes that run, and what each step and the actor are given.
- `core/hyalos/tests/run_job.rs`: the three jobs against the stand-in GitHub and a scripted model (`common::Model`). The reader's call holds the issue and no tools. The planner's holds only typed values. The actor's prompt holds none of the issue's text. Two tokens are minted and both revoked, and the log has three jobs of one task. A reader that cannot read its issue, or whose answer is not its fields, fails its job, and no sandbox starts.
- `core/broker/tests/reader.rs`: a planner's call, and a planner that names an issue.

**The actor does not see the issue's text.** That is the point of the design, and also a cost until S4.4. The actor knows the issue's number, the classification, the plan and whatever the operator wrote with `--issue`. A run on an issue whose title says all there is to know works less well than the direct run did, unless the operator describes the work. Gated handles (S4.4) are how the issue's text reaches an actor, with an approved decision bound to its digest. The operations procedure says this.

**What changes for operators:**
- A `--issue-number` run now makes two small model calls before the sandbox starts.
- It mints two forge tokens, a read-only one for the reader and the actor's.
- It needs the policy to allow `read` and `data_flow` for those jobs, which the documented policy already does.
- A step that fails ends the run early, with the step's code in the log.

**Runs are sequential.** One running job per task means a step waits for the one before it. That fits this workflow, whose steps depend on each other. Concurrent steps would need the decision ADR-0019 deferred.

**The broker's channel is busy during a step.** ADR-0061 covers this. No sandbox runs during a reader or planner job, so nothing waits behind it in this workflow.

**An interrupt during a step's call waits for the call.** The first Ctrl-C or `SIGTERM` is checked at the start of each job ([ADR-0053](0053-an-interrupted-run-ends-its-job.md)). A reader or planner call blocks in `BrokerProcess::read` until the broker answers or `STEP_WAIT` runs out, so an interrupt during a call takes effect when the call returns, within 180 s. That step's job ends as it would have. The next job is recorded and ended `failed` with `run.interrupted` before it calls anything, mints a token or starts a sandbox. A second interrupt still exits at once.

What this does not decide:
- **Installing other workflows.** Only the shipped workflow runs. Storing an operator's workflow, an install command, and choosing a workflow by name come with the first need for one.
- **Branching on values.** Every step runs. An `investigate` approach or `clear: false` is passed to the actor as a value, not used to skip it. Conditions in the workflow format would be a new version of it.
- **Per-step models, budgets and hosts in the document.** These stay in the runner. ADR-0060 says the format grows them when needed.
- **An actor's result** (S4.7) and **gated handles** (S4.4).

**Costs.** About 480 more lines of trusted core in the `hyalos` crate (the runner module and the reshaped `run_cmd`) and 10 in the broker. About 70 fewer in `hyalos-types`, with the direct run gone. Trusted core goes from 14,872 to 15,295 lines of 30,000.
