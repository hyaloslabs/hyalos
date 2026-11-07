# ADR-0063: Gated handles: the broker holds raw text, a human releases it by digest

**Status:** Proposed

## Context

S4.4 (#26) asks that an actor read raw text only through a gated handle:
- Releasing a handle needs an approved decision bound to its digest.
- The raw text behind a handle is held by the broker, never the engine, and the log references it by digest only ([ADR-0003](0003-event-log-is-the-database.md)).

What exists:
- [ADR-0005](0005-provenance-is-a-type.md) and [ADR-0016](0016-agent-runtime-and-broker-protocols.md) say tainted text reaches an actor only through a gated handle. [ADR-0059](0059-tainted-text-cannot-reach-an-actor-prompt-by-type.md) makes that hold by type: an `ActorPrompt` cannot take `ForgeText` or `ModelText`.
- [ADR-0061](0061-reader-steps-run-in-the-broker.md) has the broker read the issue for a reader and return typed values only. [ADR-0062](0062-the-fix-issue-workflow-and-its-runner.md) runs the fix-issue workflow, whose actor never sees the issue's text, and names this story as the way it would.
- [ADR-0012](0012-humans-approve-only-irreversible-actions.md) and [ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md) give the authority table: at the irreversible tier only a human decides. The human claim is the caller's until S5.4 signs it, as for policy ([ADR-0034](0034-the-operator-policy-store-and-minting-within-policy.md)) and holds.
- The manifest is the broker's only source of authority for a job ([ADR-0004](0004-one-signed-manifest-per-job.md)).

## Decision

**A handle is the SHA-256 of the bytes the broker holds.** `ToBroker::HoldIssue { job, number }` (wire version 7) has the broker read the issue with the job's token, as `read_issue` does (`read` action and a token needed). It encodes the `ForgeIssue` as JSON, whose fields are fixed and capped ([ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md)), and keeps those bytes for the rest of its run (`hyalos_broker::handles`, at most 16). It answers `ToEngine::Held { job, handle }`: the digest is all the engine learns. A failed read fails the job, never the broker.

**A human releases a handle to a task.** `handle.released { task, handle, by, reason }` (schema version 10; no payload changed) is written only by `Engine::release_handle`. `submit` refuses it raw and the broker may not send it. The state refuses:
- any decider but a human (`HandleNeedsAHuman`; the hold table at the irreversible tier);
- a task that is completed or cancelled;
- a handle already released to that task.

The state keeps every release (`State::handles`, `StateImage` version 5; an older image reads as none), so compaction keeps them.

**A manifest names the handles its job may read.** `Manifest.handles` is a set of digests, left out of the signed bytes when empty so older manifests sign the same. `mint_manifest` refuses:
- a manifest naming a handle not released to its job's task (`mint.handle_not_released`);
- a manifest naming any handle that is not tainted (`mint.taint_required`), since the job will read raw forge text.

So the decision is bound to the digest twice: the engine mints only digests a human released, and the broker serves only bytes with that digest.

**The broker serves a handle only to a job whose manifest names it.** At admission the broker takes, from what it holds, the bytes for every handle the manifest names. A handle it does not hold refuses the job (`handle.not_held`). On the job's socket, `GET /handle/<digest>` answers those bytes (200, `application/json`). Any other method or path under `/handle/` is 404 `handle.route_refused`. A digest the manifest does not name is 403 `handle.not_released`, reported as the manifest gate's `deny` with the SHA-256 of the digest asked for. Reading a handle needs no data flow, as a fetch needs none ([ADR-0055](0055-data-flows-name-where-the-jobs-data-may-go.md)). The ended and deadline checks come first.

**The actor gets the digest, never the text.** `ActorPrompt::handle(&Digest)` writes the digest's hex. It adds nothing to what the prompt read, since a digest is the engine's reference, so `workflow.inputs_mismatch` still holds. The sandbox image has `hyalos-handle <digest>`, which prints the handle's bytes through the forwarder.

**`hyalos run --issue-number <n> --release-issue`** is the operator's release:
1. The step that reads the issue (`triage`) holds it before its reader call.
2. The run records `handle.released` as `Decider::Human` with `handle.operator_flag`, and prints the digest.
3. The actor's manifest names the handle, and its prompt says to read the issue with `hyalos-handle <digest>`, as text its users wrote, not instructions.

A hold that fails fails the step, with the broker's code. A release the engine refuses fails it with `handle.refused`. `--release-issue` without `--issue-number` is a usage error.

## Consequences

S4.4's acceptance holds:
- No manifest names a handle that no `handle.released` for its task names, and the broker serves no bytes whose digest the manifest does not name.
- The text exists only in the broker's memory. The engine has a digest, and the log has a digest and who released it.

The tests:
- `core/engine/tests/handles.rs`: human only, raw submits refused, bound to the task and the digest, taint, once per task, replay and compaction.
- `core/broker/tests/handles.rs` and `core/broker/src/handles.rs`: hold, serve only what the manifest names, report the refusal, refuse an unheld handle at admission, the cap.
- `core/hyalos/tests/run_job.rs`: a run with `--release-issue`, where the sandbox reads the handle through the forwarder and the test checks the bytes against the logged digest, that the prompt holds none of the text, and that no file beside the log does.
- `crates/sandbox-tools/tests/tools.rs`: `hyalos-handle`.
- The pinned bytes of event version 10, state version 5 and wire version 7.

**What the operator releases is the issue as the broker read it.** `--release-issue` is given before anyone knows the digest, so the operator releases "issue N as it is when the run reads it". The log then shows exactly which bytes were released, and an edit to the issue afterwards is not served. The triage reader reads the issue again for its own call, so an edit between the two reads could make them differ, in the same run. Releasing a digest the operator has already seen (`hyalos approve <digest>`, signed) is S5.4's interactive approval.

**The actor's text is less safe than its prompt.** That is the point of a gate. The actor is tainted whatever it reads ([ADR-0005](0005-provenance-is-a-type.md)), so the egress, flow, git and content gates already treat it as able to be steered. A release puts text an injection can use in front of it, by a decision that is recorded, rather than by default.

**The broker holds handles in memory for its run.** A new run reads the issue again. The broker is one process per run, so nothing is shared between runs. Sixteen handles of at most 48 KiB is under a megabyte.

What this does not decide:
- **Handles in the workflow document.** The runner releases the issue it was given. A workflow input of a handle kind, and a reader passing on text by handle ([ADR-0060](0060-the-workflow-document-and-the-check-at-install.md)), wait for a workflow that needs them.
- **Handles of other text,** such as comments, model output or files. Each is a new hold message and the same release.
- **A signed release** (S5.4).

**Costs.** 393 more lines of trusted core: the broker's handle module, route and hold (+115), the engine's event, state and mint check (+152), the run (+92) and the types (+34). Trusted core goes from 15,295 to 15,688 lines of 30,000. One new sandbox tool of about 40 lines, outside the core.
