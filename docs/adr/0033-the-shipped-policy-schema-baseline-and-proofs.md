# ADR-0033: The shipped policy schema, baseline and proofs

**Status:** Proposed

## Context

[ADR-0008](0008-cedar-for-policy.md) decides that Hyalos ships a Cedar schema that fixes each action's tier, ships a baseline of `forbid` policies that operator policy cannot override, proves properties of the baseline in CI with `cedar-policy-symcc` and cvc5, compares each change with the previous version, and pins `cedar-policy` and the analysis crate together. It leaves the schema, the actions, the request shape, the baseline's rules and how the proofs are stated and run. This ADR adds them for story S3.6. It changes nothing ADR-0008 and [ADR-0012](0012-humans-approve-only-irreversible-actions.md) decided.

## Decision

**The schema** is a file in the repository (`core/policy/policies/schema.cedarschema`), included in the binary and not replaceable at run time. A request has a principal `Job` (whether it is tainted, and its one repo), a resource `Target` (its repo, whether the branch is under the job's prefix, whether it is protected) and a context of two facts: whether an approval was verified for this action, and whether the request would use a subscription token. Cedar has no prefix test, so the engine compares the branch with the prefix and passes the result. The actions are `read`, `push`, `open_pr`, `comment`, `request_host`, `merge`, `change_policy`, `publish`, `spend_over_cap` and `export_credentials`.

**Tiers are action groups.** The schema has three groups, `routine`, `escalated` and `irreversible`, and every action is a member of exactly one. Merge, change of policy, publishing, spending above a cap and exporting credentials are irreversible ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). Merge is irreversible whatever the branch, the conservative reading. Because operator policy is validated against this schema and cannot add or change an action, it cannot move one to a lower tier. A second pair of groups, `repo_write` and `branch_write`, lets the baseline name the actions that change a repo and that put commits on a branch.

**The baseline** is `core/policy/policies/baseline.cedar`, five `forbid` policies: no write outside the job's repo; no push outside its branch prefix; a tainted job cannot merge; a tainted job cannot use a subscription token; no irreversible action without an approval. Each has a stable `@id`; the loader sets the policy's id from it and refuses a baseline policy with none.

**Loading.** `Policies::load(operator_text)` builds the baseline, adds the operator's policies under ids of its own (`operator.N`, whatever they annotate, so one cannot take a baseline id), refuses templates, and validates the whole set against the schema in strict mode. A policy that fails is rejected with Cedar's reasons, before a human is asked to approve it. `Policies::parse` stays as the bare evaluator, with no schema and no baseline, and cannot answer a typed request. The engine asks with a `PolicyRequest` of typed facts. It is written out as Cedar entities with every string escaped, and checked against the schema, so a repo name cannot change the question.

**What CI proves.** Let *A* be the baseline plus the most permissive operator policy, `permit (principal, action, resource);`. A satisfied `forbid` overrides every `permit`, so anything *A* allows is an upper bound for every operator policy. For every action in the schema, the solver proves that *A* implies a policy allowing everything except the forbidden region: write outside the repo, push outside the prefix, tainted merge, tainted use of a subscription token, irreversible action without an approval. It also proves that *A* is not vacuous (it allows some request in every action, so the properties are not true of a policy that denies everything), and that no baseline policy can error on a well-formed request. A test removes each rule in turn and requires the comparison to report that more is allowed, which shows the proofs fail when a rule is missing. This is stated in terms of the schema; how the engine fills in the facts is not covered (ADR-0008).

**What CI reports.** On a pull request, `policy-diff` compares the baseline on the base branch with the new one, each with every operator permit, for every action, and adds to the job summary whether the new one allows more, less or the same, with an example request when it allows more. It reports and does not fail: a reviewer decides, as ADR-0008 says.

**Where the solver lives.** In `crates/policy-proof`, not in the trusted core, and the binary does not link it. `cedar-policy` is pinned to `=4.14.1` in `hyalos-policy` and `cedar-policy-symcc` to `=0.8.1`, which itself requires exactly that release, so the two move together. cvc5 is the release the crate documents (1.3.1), downloaded in CI against a pinned checksum. The proof tests skip with a message when there is no solver, and fail when `HYALOS_REQUIRE_CVC5` is set, as it is in CI.

## Consequences

The baseline is readable, diffable and the same in every build; changing it is a pull request that shows what it now allows. An operator can add permits and forbids and can not loosen the baseline or lower a tier. The set of actions is closed: a new one is a change to the schema, to `Action`, to the tier table and to the proofs, and a test keeps the Rust and the schema equal.

The proofs are as strong as the schema and the engine's filling in of the facts, and as the solver. They run on a release of cvc5 one or two behind the newest, because the analysis crate documents it. The job that runs them needs network access to fetch the solver. A baseline change is only as visible as the pull request's job summary, so a reviewer has to look at it.
