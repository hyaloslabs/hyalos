# ADR-0034: The operator policy store and minting within policy

**Status:** Accepted

## Context

[ADR-0003](0003-event-log-is-the-database.md) makes the log the source of truth and every projection a replay of it. [ADR-0008](0008-cedar-for-policy.md) says the operator writes Cedar policy, that it is validated against the shipped schema before anyone is asked to approve it, and that a stored policy which stops validating holds what depends on it instead of allowing it. [ADR-0012](0012-humans-approve-only-irreversible-actions.md) makes a change of policy irreversible, so a human's. [ADR-0033](0033-the-shipped-policy-schema-baseline-and-proofs.md) ships the schema and baseline and loads operator text. It leaves where the operator's text is kept, who may change it, and how the engine uses it to decide what a job may be given. This ADR adds them for story S3.13. It changes nothing those ADRs decided.

## Decision

**The policy is an event.** `policy.approved` carries the whole document, who approved it (`by`) and a short reason code. The latest one is the operator policy in force; before any, there is none and only the baseline applies. Nothing is stored beside the log, so a restore, a replay and a compaction all give the same policy. Schema version is 5 (the new kind only; no payload changed) and `StateImage` version 4 carries the policy in force, so compaction keeps it. Earlier images read as having none.

**Only the engine writes it, and only for a human.** `Engine::approve_policy` is the one way in; `submit` refuses the raw event, and the broker may not send it ([ADR-0024](0024-event-origins-and-time-order.md)). The state refuses it from anyone but a human, using the same authority table that releases an irreversible hold touching policy ([ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md)). As with holds, the engine does not verify that the caller really is a human; the operator's signature on an approval is S5.4. A document is limited to 256 KiB, in the state and so in replay.

**Rejected before it is written.** `approve_policy` loads the document with `Policies::load` first, and a document that does not parse, uses a template or does not match the schema is refused with nothing written. `Engine::check_policy` asks the same without writing, so a human is not shown a policy that would fail.

**A stale policy holds.** The log may hold a document a later build's schema refuses. Opening the engine does not fail, since that would stop the operator replacing it. `Engine::policies` returns an error naming the event, and minting treats it as unknown: the request is held with reason `policy.invalid`, never allowed. Approving a valid document replaces it.

**Minting within policy.** `Engine::mint_manifest(manifest, key, now)` signs a manifest only if its job is running, its deadline is after `now`, and the policy in force allows every capability it asks for. Each action, tool, data flow and egress host is a separate question to the policy (`use_tool`, `data_flow` and `request_host` name what they apply to; the job's token budget is a fact of the job). The first answer that is not allow ends it, as `Denied` for a deny and `Held` for unknown, so no partly allowed manifest exists. A manifest naming an action the schema does not have is refused whatever the policy says. The approval fact is never set by minting: with no approval event yet, an irreversible action cannot be minted, which is incomplete in the safe direction. Minting writes nothing to the log.

**Schema additions.** `Job.budget_tokens`, `Target.name` and the actions `use_tool` and `data_flow` (routine), so an operator can cap a budget and allow tools, flows and hosts by name. The proofs cover the two new actions.

## Consequences

The operator's policy is auditable, replayable and signed with the rest of the log, and changing it is a recorded human decision. The whole document is in the log and in every snapshot, which is why it is bounded. Because only the engine can mint and only within policy, the manifest the broker admits ([ADR-0031](0031-the-signed-manifest-and-its-admission.md)) cannot ask for more than the operator allowed. What is not here: verifying that a human really approved (S5.4), approval events for irreversible actions, and the branch facts of a particular action, which are known at the moment of the action and not at the grant.
