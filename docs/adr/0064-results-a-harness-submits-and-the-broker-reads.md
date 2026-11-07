# ADR-0064: Results a harness submits through `hyalos-submit`, and the broker reads as typed values

**Status:** Accepted

## Context

S4.7 (#84) asks that a planner or actor in a sandbox return its result through `hyalos-submit`, and that the broker check it against the step's schema:
- Input over the size or depth cap, with unknown fields, or with unbounded free text is refused and fails the job, never the broker.
- The engine receives only typed values, and the harness's chat output is discarded.
- A missing or invalid submission fails the job.
- The same helper works from Claude Code and Codex, and a result from the Claude Agent SDK or the OpenAI Agents SDK is checked the same way.

What exists:
- [ADR-0016](0016-agent-runtime-and-broker-protocols.md) decided the path: the harness runs `hyalos-submit`, which posts through the forwarder to the broker. That is decision D1 of the reference-job walk.
- [ADR-0040](0040-the-sandbox-image.md) ships `hyalos-submit` in the image. Until this story the broker answered its post with 404.
- [ADR-0060](0060-the-workflow-document-and-the-check-at-install.md) lets an actor step declare an `output`, and the format has no string field. Until now nothing used an actor's `output`.
- [ADR-0061](0061-reader-steps-run-in-the-broker.md) has the broker read a reader's answer as typed values (`workflow::read_answer`), and sends them to the engine as `ToEngine::Values`: no text, and a choice is its option's index.
- [ADR-0062](0062-the-fix-issue-workflow-and-its-runner.md) runs readers and planners in the broker, then one actor in a sandbox. Its actor had no result.
- The manifest is the broker's only source of authority for a job ([ADR-0004](0004-one-signed-manifest-per-job.md)).

## Decision

**The manifest names the result.** `Manifest.result` is an optional `workflow::Output`, the same typed fields a reader returns. It is left out of the signed bytes when absent, so older manifests sign the same. `mint_manifest` refuses one that is not a well-formed output (`mint.bad_result`, by `workflow::output_is_valid`), and the broker refuses it at admission as well (`admission.bad_result`). So what the broker checks a submission against comes from the signed document, not from the sandbox or the channel.

**The broker reads a submission as it reads a reader's answer.** On the job's socket, `POST /hyalos/submit` is read by `hyalos_broker::submit`, after the ended and deadline checks. It needs no data flow, since the values go only to the engine. The checks, in order:
- The body is at most `workflow::MAX_SUBMISSION` (16 KiB): `submit.too_large` (413).
- Nesting deeper than one object is refused before parsing: `submit.too_deep`.
- `workflow::read_submission` takes exactly the object `read_answer` does: `submit.not_json`, `submit.duplicate_field`, `submit.unknown_field`, `submit.missing_field`, `submit.bad_value`. It differs in one way: no code fence and no surrounding text, because a harness runs a command and does not chat.

An output has no string field, so a result holds no free text by construction. A choice must be one of the workflow's own options.

**One submission per job, kept until the engine takes it.** The first submission, accepted or refused, is the job's (`submit::Submitted`). Any later one is 409 `submit.already` and changes nothing. A path under `/hyalos/` other than the submit path, a method other than POST, and a job whose manifest names no result are 404. The broker reads the declared body of a submission that is too large, up to a bound, so the harness gets the 413 answer rather than a reset connection.

**The engine takes the result after the sandbox ends.** `ToBroker::TakeResult { job }` (wire version 8) is answered with `ToEngine::Values` (one per field, in the output's order) or `Refused`:
- `submit.missing` when nothing was submitted;
- the refused submission's code;
- `submit.not_expected` when the manifest names no result.

A refusal ends the job in the broker, which serves on. The run reads the answer through the same loop that records gate reports, so a report that arrives late is still recorded. It checks the values again with `values_fit`.

**`hyalos run` gives the fix-issue actor a result.** The shipped workflow's `fix` step now has an output: `outcome` (`pr_opened`, `no_change` or `blocked`) and `tests_added`. The actor's manifest names it, and the engine's steps in its prompt end with the fields, what each may hold, and an example `hyalos-submit` command (`workflow_run::submission`). When the harness exits 0, the run takes the result:
- If the values fit, the run prints them as the workflow's options (`result: outcome=pr_opened, tests_added=true`), and the job ends `succeeded` with `step.submitted`.
- If not, the job ends `failed` with the broker's code, or with `submit.no_answer` when the broker gave nothing that fits.

`hyalos run --issue` alone runs one actor with no result, as before.

**The harness's output is not its result.** The run never parses the output, never records it, and never passes it to a step. For a job with a result, the run shows the output in the operator's terminal only when the job did not succeed, so a failure can be diagnosed. A job with no result shows it as before.

**The path depends on the manifest, not the role.** A planner in a sandbox would submit the same way. This runner still runs planners in the broker ([ADR-0062](0062-the-fix-issue-workflow-and-its-runner.md)), so the read-only sandboxed planner of ADR-0016 waits for a workflow that needs one.

## Consequences

S4.7's acceptance holds:
- **Submission and checks.** The actor submits through the forwarder, and the broker checks it against the step's output: size, depth, unknown and missing fields, kinds, ranges and options.
- **Failure.** An invalid submission and a missing one each fail the job. The broker refuses, forgets the job and serves on.
- **What the engine gets.** The engine receives typed values only. The harness's output is never the result.
- **Harnesses.** Claude Code (its Bash tool) and Codex (its shell) each run `hyalos-submit` in the image, and the broker reads the same values from both.
- **Agent SDKs.** A submission is one HTTP post that any runtime can send by running a command. The Claude Agent SDK and the OpenAI Agents SDK are not in the image, so no test runs them. `core/broker/tests/submit.rs` sends plain posts, which is all either SDK would send through the helper.

The tests:
- `core/types/src/workflow.rs`: `read_submission` against `read_answer`, the codes and the cap.
- `core/types/tests/manifest.rs`: the result is signed when present and left out when absent.
- `core/engine/tests/policy_store.rs`: `mint.bad_result`.
- `core/broker/src/submit.rs` and `core/broker/tests/submit.rs`, over a real job socket:
  - typed values only, and one submission per job;
  - each refusal fails the job, the broker serves on, and `submit.missing`;
  - no result in the manifest, a wrong path, a passed deadline and an unknown job;
  - a planner submits the same way;
  - a malformed result is refused at admission.
- `core/hyalos/tests/run_job.rs`:
  - the workflow run submits, the result is printed, the job ends `step.submitted`, and the harness's printed text is not shown;
  - a bad value, an unknown field, text that is not JSON and no submission each fail the job with the broker's code, and the output is shown;
  - the released-handle run submits too.
- `crates/sandbox-tools/tests/tools.rs`: `hyalos-submit` posts the result, fails on a refusal, and refuses past 16 KiB before sending.
- `crates/sandbox-tools/tests/submit.rs`: Claude Code and Codex in `hyalos-sandbox:test` each submit when the model asks them to, and submit nothing otherwise. Each run is checked against what the broker gives.
- The pinned encoding of `take_result`, and wire version 8.

**A harness that never submits runs until it exits.** The broker learns of an invalid submission at once, but the run asks only after the sandbox ends. A refused harness can keep calling its model until it exits, within its budget and deadline. Stopping the sandbox on a refused submission would need a report sent unprompted, and can come later if runs show the need.

**ADR-0040 said the submit path answers 404.** That stopped being true with this ADR. ADR-0040 is Accepted, so its text stays as it is.

What this does not decide:
- **A sandboxed planner** (ADR-0016's read-only variant) and a runner that passes an actor's result to a later step. The runner still runs one actor, last, and its result goes to the operator.
- **Returning results to a driving agent** (S5.2, S5.10). The values are ready for that, as typed values and the workflow's options.

**Costs.** 302 more lines of trusted core:
- the broker's submit module, route and `TakeResult` (+132);
- the run's result, prompt and output handling (+126);
- the types' manifest field, wire message and submission reader (+34);
- the engine's mint check (+10).

Trusted core goes from 15,688 to 15,990 lines of 30,000. Outside the core, `hyalos-submit` gains a size check, and the image tests' scripted model can ask a harness to run a command.
