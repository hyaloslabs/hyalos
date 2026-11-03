# ADR-0029: Cancelling a held task, and recording a refusal

**Status:** Proposed

## Context

[ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md) lets a hold be released by a fresh on-time allow or by a decision at its tier, and says a refusal does not release it. That left two gaps found while building it. A held task could only ever be released, never ended: there was no way to say "this will not happen". And a gate that answered deny when a hold was asked again changed nothing and left no trace, so the hold stayed open and nothing showed the gate had refused.

This ADR adds to ADR-0025 and changes nothing it decided.

## Decision

**Only a recorded decision ends a task; a refusal alone never does.** A gate's deny says the action is refused now. It may be a policy that is being fixed, a gate that is misconfigured, or a transient answer, and ADR-0009 already treats the gate's answer as evidence and not as a verdict on the task. If a deny could end a task, a broken or compromised gate could destroy any held work by answering deny. So a refusal leaves the hold open and the task held, as before, and a decider decides what happens next. A later fresh on-time allow still releases the hold, unless the task was cancelled.

**A refusal is a fact: `hold.refused`.** When a held request is put to its gate again and the gate answers deny, `Engine::reevaluate_hold` writes a `hold.refused` event (the hold and the gate's reason code) and returns `Reevaluation::Refused`. It is the engine's, written only through that call, for an open hold and its own request. Unknown, a timeout and a late allow still write nothing: they say the gate did not answer, which is not evidence of anything. The event changes no state, so it is history in the log, and after a compaction it is in the archive with the rest of the events it was written with; it is not carried in the snapshot.

**A held task is ended by `task.cancelled`.** `Engine::cancel_task(task, decider, reason)` writes it, with who decided (policy, the verifier agent or a human) and a reason code. Afterwards the task has no open holds and nothing can start on it: a job, a completion and a new hold on it are refused, and a later allow from the gate or a decision on its holds finds them not open. Its holds are not marked released: they were ended, not released, which is a different thing for anyone reading the log, so they stay unresolved and the task says why they are no longer open. Only a held task can be cancelled; an open, completed or already cancelled task is refused and nothing is written.

**Cancelling needs the same decider as releasing, at every open hold.** A human may cancel any held task. Policy may cancel a task whose holds are all routine and touch nothing sensitive, the verifier agent one whose holds are all escalated and touch nothing sensitive, and nothing sensitive (credentials, policy, publishing, a capability grant) or irreversible can be cancelled by anyone but a human. A task with several holds needs a decider every one of them allows, so a task holding a routine and an escalated hold is a human's. The difference from releasing is one column: a gate answering again is never a decision to end a task, so it is not a decider here.

This is a judgment and the alternative is worth stating. Ending a task is a safer direction than releasing it, since the held action is never taken, which argues for a lower bar. It is not lower here because the tier says who is entitled to decide this action, a decider below it would be overruling the tier by choosing the cheap outcome (a verifier cancelling what policy was to release, say, or an agent cancelling what only a human may approve, which also destroys work the human was meant to be asked about), and one table is one thing to check and one thing to test. A deployment that wants a lower bar for cancelling writes a new ADR; it is a one-line change in one function (`Stakes::may_be_cancelled_by`).

**There is no other way in.** `submit` and `submit_all` refuse `task.cancelled` and `hold.refused` from the engine, so each is written only by the call that justifies it, and the broker may submit neither ([ADR-0024](0024-event-origins-and-time-order.md)). The state refuses a cancellation by a decider the stakes do not allow, and replay applies the same check, so a validly signed log that cancels a task the wrong way does not open.

**Schema version 4, state image version 3.** The two new kinds change no existing payload, so no step is needed for events; an older build refuses a log that holds one. The snapshot's state image gains each task's cancellation, and an image from version 1 or 2 is read as having none ([ADR-0026](0026-compacting-the-log-by-signed-snapshot.md)).

## Consequences

A task held for a human can now be ended by one, and the record says who and why. A deny leaves a trace without becoming a decision.

The decider is not authenticated here either, as in ADR-0025: the record names the tier of decider, and the channel that calls `cancel_task` establishes that it was a human.

A task cannot be un-cancelled. The same work is a new task.

A gate that keeps refusing writes a `hold.refused` event each time it is asked, so a loop that asks without limit grows the log without limit. The engine does not rate-limit it; the caller that decides when to ask again does, and a hold nobody decides is still held for as long as it takes, as in ADR-0025.

The refusal facts are not in the snapshot, so after the next compaction they are only in the archives, and an operator who has pruned those no longer has them. Whether a task is held, and who ended it, are in the snapshot.
