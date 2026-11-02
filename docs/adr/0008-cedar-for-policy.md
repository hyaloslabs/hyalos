# ADR-0008: Cedar for policy

**Status:** Accepted on November 1, 2026, after review against ADR-0003, 0004, 0005, 0009, 0012 and 0013 and Cedar's documented semantics. The clarifications from that review are folded in below.

## Context

Manifests, install policy and approval tiers are all policy. Hand-written checks are hard to reason about as a whole.

## Decision

Express manifest constraints, install policy and the allow-or-deny part of approval tiers in Cedar, evaluated by the engine. Use Cedar's automated analysis in CI to prove properties of the policies, such as "no policy lets a tainted job merge to a protected branch."

**Where it runs.** The engine evaluates Cedar when it mints a manifest, decides an install and classifies a request. The broker does not evaluate Cedar: it enforces the signed manifest and nothing else ([ADR-0004](0004-one-signed-manifest-per-job.md)). Checks that need bytes, such as the content gates, stay in code.

**Tiers are fixed by the schema.** Each action belongs to a tier in the schema that ships with Hyalos. Operator policy decides allow or deny within a tier and cannot move an action to a lower one, so an irreversible action always needs a human approval ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). The engine sets the approval fact in a request only after verifying the operator's signature on the approval event ([ADR-0003](0003-event-log-is-the-database.md)).

**A baseline that operator policy cannot override.** Hyalos ships a baseline of `forbid` policies that is always loaded and that operator policy cannot remove. In Cedar a satisfied `forbid` overrides every `permit`, whatever the order of the policies ([Cedar authorization](https://docs.cedarpolicy.com/auth/authorization.html)). What the baseline forbids therefore stays forbidden under every operator policy, including ones written after CI ran. Operator policy adds permits and further forbids on top.

**What CI proves.** Cedar's symbolic analysis (the [`cedar-policy-symcc`](https://docs.rs/cedar-policy-symcc) crate, which needs the cvc5 solver) proves these properties of the baseline:

- no manifest grants a write outside the job's own repo and branch prefix ([ADR-0004](0004-one-signed-manifest-per-job.md));
- a tainted manifest cannot merge, push outside its branch prefix or use a subscription token ([ADR-0004](0004-one-signed-manifest-per-job.md), [ADR-0005](0005-provenance-is-a-type.md));
- no irreversible action is allowed without an approval ([ADR-0012](0012-humans-approve-only-irreversible-actions.md));
- no baseline policy can error on a well-formed request (see "Errors hold the task").

CI also compares each change to the shipped default policies with the previous version, so a change that allows more than before is visible in review.

**What the proof does not cover.** It proves properties of the policies over the schema. It does not prove that the engine fills in the request attributes correctly (taint, protected branch, approval, repo). That rests on types ([ADR-0005](0005-provenance-is-a-type.md)), tests and the simulation invariants ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)).

**Errors hold the task.** Cedar skips a policy whose evaluation errors and reports it only in the diagnostics, so a `forbid` that errors, for example on a missing attribute, can let a request through ([Cedar operators](https://docs.cedarpolicy.com/policies/syntax-operators.html)). The engine treats any evaluation error, and any policy that fails validation against the schema, as unknown ([ADR-0009](0009-gates-answer-allow-deny-unknown.md)), which holds the task. It never treats either as allow. Every policy is validated against the schema when loaded, and an operator policy that fails validation is rejected before a human is asked to approve it.

**Policy changes** follow [ADR-0003](0003-event-log-is-the-database.md) and [ADR-0012](0012-humans-approve-only-irreversible-actions.md): a human approves the validated document, it is stored as an event, and the policy in force is the projection. The shipped defaults live in the repository.

## Consequences

Policies are data people can read and diff. Some checks stay in code: the byte-level content gates and the broker's per-request checks against the manifest. Cedar's evaluator becomes a dependency of the trusted core. The analysis crate depends on an exact `cedar-policy` release, so the two are pinned together, and the cvc5 solver runs in CI only, never in the binary. A proof is only as good as the schema and the engine's population of attributes. Evaluation is a pure function of the policies, the entities and the request, and any time comes from the engine's clock, so the simulation ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)) runs it unchanged.
