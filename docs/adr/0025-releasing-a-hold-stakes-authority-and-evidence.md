# ADR-0025: Releasing a hold: stakes, authority and evidence

**Status:** Proposed

## Context

[ADR-0009](0009-gates-answer-allow-deny-unknown.md) says unknown never becomes allow by itself: a hold resolves when its gate is asked again and can answer, or by a decision at the tier of the action. [ADR-0012](0012-humans-approve-only-irreversible-actions.md) names the tiers and says a verifier agent can never approve credentials, policy, publishing or a capability grant. [ADR-0020](0020-gate-answers-timeouts-and-holds.md) built the hold and left its release open.

Until now the engine accepted a `hold.resolved` event from the engine with no condition at all, and the event said only who, not why. A hold also did not record how much its action mattered, so nothing could tell a routine hold from one that needs a human.

## Decision

**A hold records its stakes when it opens.** `hold.opened` carries a tier (routine, escalated, irreversible, as in ADR-0012) and the set of sensitive categories the action touches (credentials, policy, publishing, capability grant). The engine decides the stakes from the action and puts them on the gate request; nothing the broker or a sandbox reports can lower them. How the engine works them out from a manifest comes with the manifest ([ADR-0004](0004-one-signed-manifest-per-job.md)); until then the caller of the gate supplies them, and a hold nobody classified is the most cautious.

**Who may release a hold is a function of its stakes,** one definition (`Stakes::may_be_resolved_by`) that both the engine and replay use:

| Stakes | Policy | Verifier agent | Fresh gate allow | Human |
| --- | --- | --- | --- | --- |
| routine | yes | no | yes | yes |
| escalated | no | yes | yes | yes |
| irreversible | no | no | no | yes |
| touches anything sensitive, at any tier | no | no | no | yes |

Each tier has its own decider, so a stronger one does not stand in for a weaker one by accident; a human can decide any hold. A fresh allow is the gate's own answer, which ADR-0009 already counts as resolution, but not for a hold that needs a decision, since a gate allow is not a human's consent.

**Re-evaluation releases only on a fresh, on-time allow.** `Engine::reevaluate_hold` takes the outcome of `ask` for the hold's own gate and inputs. A request for another gate or other inputs is refused. Only an on-time allow writes anything, a `hold.resolved` recorded as resolved by re-evaluation with the gate's reason code. Unknown, a timeout, a late allow, a gate failure and a refusal leave the hold open and write nothing. A refusal does not release it: only an allow or a decision does.

**A decision is recorded with who made it.** `Engine::decide_hold` takes the decider (policy, the verifier agent, a human) and a short reason code. Every `hold.resolved` now carries `by` and `reason`. Reasons are codes, never free text ([ADR-0020](0020-gate-answers-timeouts-and-holds.md)).

**There is no other way in.** `submit` and `submit_all` refuse a `hold.resolved`, so every release goes through one of the two calls above. The state also refuses a release by anyone not allowed at the hold's stakes, and replay applies the same check, so a validly signed log that releases a hold the wrong way does not open.

**A hold does not extend a job's deadline.** A hold ends its job at once ([ADR-0020](0020-gate-answers-timeouts-and-holds.md)), so no job runs while a task is held, and releasing the hold revives nothing: the held job stays ended and a new job, with its own manifest and deadline, is what runs next. Time passing releases nothing. A gate answer that arrives for a job that has already ended, for a deadline or any other reason, is refused: it cannot hold the task and it cannot allow. Ending a running job when its manifest's deadline passes belongs to the manifest story.

**Schema version 2.** The two event shapes change, so the version moves to 2 and a version 1 event is upcast on read ([ADR-0023](0023-event-schema-versions-and-upcasting.md)): a version 1 hold gets the most cautious stakes (irreversible, nothing touched), and a version 1 resolution gets the reason `legacy.unrecorded`.

## Consequences

A hold cannot be released by a bug or a compromised component that only has the ability to append: the release must name a decider the stakes allow, and the gate path must present a real on-time allow for that hold's own request.

The ADR does not authenticate the decider. `decide_hold` records that a human decided; that it was a human (the operator's approval key, [ADR-0012](0012-humans-approve-only-irreversible-actions.md)) and which verifier agent run decided are established by the channels that call it, which are later stories. The record names the tier of decider, not the individual.

A log written by the first build, in which anyone but a human released a hold, does not open: its holds read as irreversible, and nothing shows the release was allowed. No release wrote such a log, because the events are new in this milestone. After a release, an old log that fails this way is investigated, not repaired.

The matrix is a judgment, and a change to it is a new ADR: routine by policy, escalated by the verifier, and a human for the rest follows ADR-0012 literally; letting a fresh gate allow release a routine or escalated hold follows ADR-0009. Allowing a stronger decider to stand in for a weaker one (a verifier on a routine hold, say) would be a smaller list to check but a weaker audit trail, so it is not allowed.

A gate that keeps answering unknown holds the task until someone decides. The engine does not give up on a hold by itself, and does not time it out: the operator sees it and decides.
