# ADR-0047: Forge content as typed values, and taint derived from the step

**Status:** Accepted

## Context

A reference job starts from an issue. Issue text is written by anyone who can open one, so it is the most direct route for a prompt injection.

Three ADRs bear on it:
- **[ADR-0002](0002-one-binary-two-processes-sandboxes.md):** untrusted input is parsed in the broker, under a size cap, a depth cap and the rejection of unknown fields. A validation failure fails the job, never the broker process. The engine receives only typed values, never raw forge content.
- **[ADR-0005](0005-provenance-is-a-type.md):** every value carries its provenance, and any actor that reads forge-authored content runs tainted. The contents of a repository count as forge-authored, so every actor reads some (decision D2 in `docs/planning/reference-job-walk.md`).
- **[ADR-0004](0004-one-signed-manifest-per-job.md):** taint is derived by the engine, never requested, and a tainted manifest cannot push outside its branch prefix, merge, or use a subscription token.

None of this was built. `Manifest.tainted` was whatever its caller set, nothing read the forge, and the channel ([ADR-0035](0035-the-private-channel-between-engine-and-broker.md)) carried no forge content. These are stories S3.7 (#67) and S3.14 (#85).

## Decision

**Provenance is a type** (`hyalos_types::provenance`):
- `Provenance` has four classes: operator, forge-authored, plugin-authored, and model output derived from tainted input.
- `ForgeText` is forge-authored text. Its `Debug` form shows only its length, and reading it is an explicit `as_str`.
- `ForgeIssue` is an issue: number, state, title, body, author login and labels, each text a `ForgeText`.

An issue has caps:
- the title is at most 1 KiB, the body 32 KiB, and the author and each label 128 bytes;
- it has at most 32 labels;
- the whole is at most 48 KiB as JSON, so it fits a channel frame however its text escapes.

A body over its cap fails the read; it is never cut short. The same caps and `deny_unknown_fields` apply when an issue is read back from JSON, so the engine checks what it receives just as the broker checked what it built.

**The broker reads issues, over GitHub's GraphQL API** (`hyalos_broker::forge`).
- The engine sends `ToBroker::ReadIssue { job, number }`. The broker reads the issue from the manifest's repository and answers `ToEngine::Issue { job, issue }`.
- The read needs the `read` action (`forge.action_not_allowed`) and the job's forge token (`forge.no_token`, [ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md)). The number must be between 1 and GraphQL's largest `Int` (`forge.bad_request`), and the job must be live and within its deadline.
- GraphQL answers with exactly the fields the query names, so here, unlike in GitHub's REST answers, an unknown field is genuinely unexpected and can be refused.

The answer is checked in this order:
1. **The status.** 401 and 403 are `forge.upstream_refused`; any other status but 200 is `forge.upstream_status`.
2. **The size.** At most 256 KiB is read (`forge.too_large`).
3. **The depth.** The bytes are scanned, outside strings, for nesting of objects and arrays deeper than 16 (`forge.too_deep`). The query needs seven. This runs before any parser, so no answer can make one recurse deeply.
4. **The parse,** against closed types that refuse any field the query did not ask for (`forge.unknown_field`). The top level allows `data`, `errors` and `extensions`, which the GraphQL specification defines. Any other mismatch is `forge.malformed`.
5. **What it says.**
   - Any `errors` is `forge.refused`, and the error text is never read.
   - A missing repository is `forge.not_found`, and a missing issue `forge.no_such_issue`.
   - A state other than `OPEN` or `CLOSED` is `forge.malformed`.
   - More labels than were returned is `forge.too_large`, since a partial list would misstate the issue.
6. **The issue's caps** (`forge.too_large`).

**A failed read fails the job, never the broker.** The broker forgets the job, exactly as at `end_job`: its manifest, its forge token, its socket and its meter. It answers `ToEngine::Refused { job, reason }`, which now means that the job's manifest was not admitted or that something the broker did for the job failed. Either way the job has failed. The broker goes on serving every other job. A reason code never carries anything from the answer.

**The channel is version 4.** It adds `read_issue` and `issue`. `ForgeIssue` is the one place forge-authored text crosses the channel. It crosses as fixed, capped fields whose type says where the text came from, which is what "typed values, never raw forge content" means here. Every other string the broker sends is still a reason code. Forge content never enters the log ([ADR-0003](0003-event-log-is-the-database.md)): nothing here writes an event.

**The engine derives taint and will not sign an untainted manifest where taint follows.**
- `Engine::mint_manifest` now takes `StepInputs`, the provenances the step is given.
- `StepInputs::tainted(role)` is true for an actor always, since it reads its repository. For any other role it is true when the step is given forge-authored content or model output derived from it.
- When it is true and the manifest is not tainted, minting refuses with `mint.taint_required`.
- A tainted manifest for a step that would not need one is minted, since it can do less.
- Plugin-authored and operator values do not taint. ADR-0006 bounds plugin-authored values by capabilities instead.

**What a tainted manifest cannot do** already holds, and is now tested end to end:
- **Merge:** the baseline's `baseline.tainted_no_merge` refuses it at minting, even under an operator policy that permits everything and with an approval in the manifest.
- **A subscription token:** `baseline.tainted_no_subscription_token` refuses any request that would use one, and the broker never holds one ([ADR-0016](0016-agent-runtime-and-broker-protocols.md)).
- **Pushing outside its branch prefix:** the git proxy refuses it for every manifest ([ADR-0046](0046-the-git-proxy.md)), and its test job is tainted.

**The content gate is not built here.** No ADR yet says what the broker's content gate checks in a tainted job's outbound text, or where. Today that text is either inside compressed git packs or does not exist yet: pull request text arrives with S3.12 (#79). It is its own story, with its own ADR: S3.14b (#183), split from #85.

## Consequences

An actor's manifest is always tainted. Every manifest the reference job mints for an actor therefore carries the baseline's taint rules, which is what D2 decided. A reader given an issue is tainted too, so a step that wants an untainted manifest has to be given only operator values. That is the shape ADR-0005 asks for.

The engine can now hold issue text, as `ForgeText`, and has to keep it out of the log and out of any untainted step. The engine still has no loop that sends `read_issue` or receives `issue`; that comes with the end-to-end job (#70). A read blocks the broker's channel while it runs: up to the transport's 30-second connect and 300-second read timeouts. Moving it off the channel is left for when several jobs run at once.

Only GitHub issues are read. Comments, pull requests, repository files read through the API, GitHub Enterprise Server's GraphQL path and GitLab are not built. A GitHub change that adds a field to these exact types fails reads until this build is updated. That is the price of refusing unknown fields, and it fails closed.

The broker's tests answer the issue query with a stand-in for GitHub's GraphQL API. They cover a good issue, every failure above, an answer nested 200,000 levels deep, and every prefix and one-byte change of a real answer, which yields an issue or a reason code and never a panic. A test also checks that the broker still serves the next job after each failure, and that the issue crosses a real channel and is checked again on the engine's side.
