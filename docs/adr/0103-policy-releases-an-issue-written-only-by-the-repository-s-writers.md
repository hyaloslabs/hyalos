# ADR-0103: Policy releases an issue written only by the repository's writers

**Status:** Accepted

## Context

S4.9 (#266). Under [ADR-0078](0078-agent-started-runs-wait-for-the-operator-to-release-the-issue.md), a run the driving agent starts holds its issue, and the actor waits until the operator runs `hyalos release <task>`. So every agent-started job waits on a person, even when the repository's own maintainer wrote the issue.

The release exists for text from someone outside the repository: a stranger's issue on a public repository, written to steer the actor. Text that only people who can already push to the repository wrote is not that threat, since those people could change the code directly. The actor runs tainted whatever it reads ([ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md)), so releasing such an issue by policy widens nothing the actor may do.

What stood in the way:
- [ADR-0063](0063-gated-handles-for-raw-text.md) makes `handle.released` a human's decision only: the state refuses any other decider, at the irreversible tier of [ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md)'s table.
- The engine knows nothing about who wrote a held issue. The broker reads it, and `ForgeIssue` has its author's login as forge text, which the engine never reads.
- [ADR-0085](0085-the-capability-vocabulary.md) says the policy schema's actions are exactly what the capability vocabulary names. A release is not something a job holds.

GitHub's `authorAssociation` is not the fact we need. For an organization, `MEMBER` means a member of the organization, not someone who can write to this repository. And the issue's author is not its only writer: anyone with triage rights can edit its body or rename it.

## Decision

**The broker reads who wrote a held issue, as permissions only.** The issue query asks GitHub for three things in the same GraphQL request as the issue, so all of them describe the same text:
- the issue's author;
- every editor of its body (`userContentEdits`);
- everyone who renamed it (`RenamedTitleEvent` actors).

Each comes with its `__typename`. For each distinct `User` among them, the broker asks `GET /repos/{owner}/{repo}/collaborators/{login}/permission` with the reading job's token, and takes only the `permission` field. That field is GitHub's legacy base role: `maintain` reads as `write`, and `triage` as `read`.

Each writer becomes a `Permission`: `admin`, `write`, `read`, `none` or `unknown`. A writer is `unknown` in any of these cases:
- a deleted account;
- an actor that is not a `User`, such as a bot or an app, so one cannot pass as a user of the same login;
- a login GitHub does not allow;
- more than 16 body edits or title renames, or an answer without those lists;
- more than 8 writers besides the author, when the broker reports one `unknown` editor and looks up none of them;
- a lookup that fails, is refused or is not understood.

Logins are compared without case, as GitHub compares them.

The answer is `Writers { author, editors }`: the author's permission, and the permission of every other writer, deduplicated, each once. No login leaves the broker. `ToEngine::Held` carries the writers (wire version 11).

Labels are not counted as writing. Their names come from the repository's own label set, which only writers can change.

**Everyone who wrote it can write to the repository, or a person releases it.** `Writers::all_can_write` holds when the author and every editor are `admin` or `write`. That is the stakes [ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md) asks for:
- a held issue written only by the repository's writers is **routine**, which policy may decide;
- any other held issue stays **irreversible**, which only a human decides.

The state applies that table to `handle.released`. A human may release any held text, as before. Policy may release only text whose writers the event carries, all of whom can write. No other decider releases: not a verifier, and not a re-evaluation. Replay applies the same check, so a log that releases a stranger's issue by policy does not open.

**The operator's Cedar policy turns it on.** The schema gains the action `release_issue`, in the routine tier, and the fact `writers_only` in every request's context. The engine sets `writers_only` only when it asks `release_issue` with writers that all can write. The baseline gains a forbid, with a proof in CI like the others:

```cedar
@id("baseline.release_needs_writers")
forbid (principal, action == Action::"release_issue", resource)
unless { context.writers_only && resource.repo == principal.repo };
```

So no operator policy can release an issue anyone else wrote. Since Cedar denies by default, nothing is released by policy unless the operator's policy permits it, for example:

```cedar
permit (principal, action == Action::"release_issue", resource);
```

The starter policy does not permit it. A policy that permits every action, or every action of the routine tier, permits it too. That is the point of a tier, and the baseline's rule still holds under such a policy.

**A run that waits tries policy first.** When a run with `await_release` has its issue held, it calls `Engine::release_issue_by_policy` before writing the `.held` file. The engine method:
1. asks the policy `release_issue` for a tainted job on the run's repository, with `writers_only` from the writers;
2. on an on-time allow, records `handle.released` with `by: policy`, the reason `handle.policy_released` and the writers;
3. on anything else, writes nothing and returns the gate's code. A deny, unknown, a policy that cannot load and writers who cannot all write are each just that.

On a release the actor runs at once, as after `hyalos release`. Otherwise the run says why policy did not release it and waits for the operator, as [ADR-0078](0078-agent-started-runs-wait-for-the-operator-to-release-the-issue.md) says. `--release-issue` is unchanged, and a run with neither flag never releases the issue.

**The agent and the operator are told which kind of release happened.** The run view gains the task's release: who decided it and its reason code.
- `job_status` and `tasks/get` say that the issue was released by policy, or by the operator.
- `list_approvals` adds `released`: the runs whose issue was released, with the task, the issue's number, the decider and the reason.
- The run prints `released to the task by your policy (handle.policy_released)`.
- The set-up skill tells the operator how to turn this on, and what it means.

**Schema version 21.** `handle.released` gains `writers`, which is absent for a human's release. A payload written before reads as it was. [ADR-0063](0063-gated-handles-for-raw-text.md)'s rule that only a human releases a handle is superseded by the table above. [ADR-0085](0085-the-capability-vocabulary.md)'s rule that the schema's actions are exactly the capability vocabulary is superseded too: they are, except `release_issue`, which no job holds and no manifest names.

## Consequences

An issue the maintainer wrote reaches the actor without a person in the loop, once the operator's policy says so. An issue a stranger opened or edited still waits for `hyalos release`, and no operator policy can change that, because the baseline forbids it.

The broker makes one REST call per distinct writer, at most 8, with the reading job's token, which needs only the metadata permission every installation token has. A lookup that fails costs the release and nothing else: the run waits for the operator.

The facts are those of the moment of the read, and the digest binds the release to that text. An issue edited after the read is a different text that nobody released. A writer whose access is revoked later does not undo a release already made, as with any decision.

The permissions are bound to the repository through the reading job's manifest, which names the run's repository: the broker looks them up there. The engine asks Cedar with that same repository as principal and target, so the baseline's repository condition is defence in depth.

A Projects draft issue keeps its text when a writer converts it, and GitHub then names the writer as its author. Text a non-writer put in the draft would be released as the writer's. An operator whose project members are not all writers should leave this off.

Comments are not part of a held issue. If they are added, each comment's author is a writer under the same rule.

`hyalos github check` and token minting refuse `release_issue` as a manifest action, as they refuse every action no job holds.

Costs: 405 lines of trusted core:
- `Permission`, `Writers`, the wire message, the event's field and the schema version in `hyalos-types` (+53);
- the action, the fact and the baseline rule in `hyalos-policy` (+12);
- the state's rule and `release_issue_by_policy` in `hyalos-engine` (+61);
- the query, its parse and the permission lookups in `hyalos-broker` (+166);
- the runner, the run view, `job_status` and `list_approvals` in `hyalos` (+113).

Trusted core goes from 28,017 to 28,422 lines of 30,000.

Tests:
- `core/types`: who can write, the base roles read exactly, `Held`'s pinned encoding, and version 21's pinned `handle.released` by policy.
- `core/policy/tests/shipped.rs`: the baseline forbids a release of text anyone else wrote, or to another repository, under any operator policy, approved or not; a permitted release of writers' text is allowed; `release_issue` is in the routine tier; the vocabulary is the schema's actions but its decisions.
- `crates/policy-proof`: the new property holds for every action, and fails when its rule is removed.
- `core/engine/tests/handles.rs`: off unless the operator's policy permits it; permitted, an author or editor who can only read, has no access, or is unknown leaves nothing written; writers' text is released by policy with its writers, once, and replays.
- `core/broker/tests/forge.rs`: the writers are read as permissions only, each once and without case. A rename by a triager, an edit by a reader, a bot with a maintainer's login, a deleted editor, an author with no access, more edits than were shown, and an answer without the edits are each what the rules above say.
- `core/hyalos/tests/run_job.rs`: with the operator's policy permitting it, an issue its writer opened is released by policy, unsigned and with its writers, and the actor reads it through its handle with no `hyalos release`. An issue whose author can only read waits for the operator under the same policy.
- `core/hyalos/src/mcp_cmd.rs`: the task's status says who released its issue.
