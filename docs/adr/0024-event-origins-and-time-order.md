# ADR-0024: Event origins and time order

**Status:** Proposed

## Context

[ADR-0018](0018-event-log-format-and-verification.md) stores an `origin` with every event, `engine` or `broker`, and [ADR-0003](0003-event-log-is-the-database.md) says the broker has no key and reaches the log only through the engine. Nothing yet says which origin may submit which kind of event, so a broker that is compromised, or a bug in the code that relays its messages, could submit a `task.completed` or a `hold.resolved` and the engine would record it. The review of the projections found this, and also that nothing stops an event being stamped earlier than the one before it.

Time has a second problem. [ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md) keeps the engine from reading the clock itself, so the caller supplies each event's time, taken from a `Clock`. A wall clock can step backwards (a time-sync correction, a manual change), so taking time only from the `Clock` does not by itself make log time ordered.

## Decision

**The broker may submit only `gate.answered`. Every other kind is the engine's.** The broker evaluates gates per request ([ADR-0009](0009-gates-answer-allow-deny-unknown.md)) and reports each answer; the log records that as a broker event, "what the broker said" ([ADR-0018](0018-event-log-format-and-verification.md)). Creating and completing tasks, starting and ending jobs, and opening and resolving holds are decisions of the engine: it is the process that launches work, enforces the deadline, and hears the operator. So a compromised broker cannot finish work it did not run, end a job as succeeded, or release a hold it caused. The engine may also write `gate.answered` itself, for gates it evaluates (a verifier or a human, [ADR-0012](0012-humans-approve-only-irreversible-actions.md)).

**The rule is a function of the event and the origin only,** `Event::may_come_from`, in `hyalos-types`, so there is one definition. It is checked before the state rules, so a refused origin never reveals anything about the state.

**An unknown answer's consequences are the engine's.** When a gate answers unknown, the answer is recorded with the origin that gave it, and the job ending held and the hold opening are recorded as engine events, because the engine decides them ([ADR-0020](0020-gate-answers-timeouts-and-holds.md)). They are still written in one transaction, with a per-event origin.

**Log time does not go backwards.** An event may share a time with the one before it (events written together share one stamp) but may not be stamped earlier. The engine refuses an earlier stamp before writing anything, with the two times in the error; it does not clamp or rewrite the time, because a log whose times were adjusted silently is weaker evidence than a refusal an operator can see. If the wall clock steps back, the engine stops accepting events until the clock passes the last stamp, or the operator corrects it. The engine still takes its times from its caller, and the caller from a `Clock` ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)); this check is the backstop.

**Replay applies both rules.** Opening a log whose events break either rule, even if validly signed, fails with the event's number, as it does for any other illegal event ([ADR-0019](0019-typed-events-and-in-memory-projections.md)). Logs written under these rules never contain such an event.

**Opening has no gap between testing and acting.** `Engine::open` does not test whether the log exists and then open it. It opens the log, creating it if it is new, and verifies the file it opened (through the same connection, not by path again). A new log is empty and verifies trivially.

## Consequences

A relay bug or a compromised broker can do exactly one thing to the log: add gate answers, which the engine's state rules already constrain (a running job, an unknown answer owing a hold). It cannot move a task, job or hold along.

The rule is deliberately narrow and will need a decision when the broker gains a legitimate new event (for example, a metering event for a job's budget): that event kind is added to the closed set with its allowed origin in the same change.

Origins record who gave the information, not proof of it. A broker-origin gate answer is still only what the broker said ([ADR-0018](0018-event-log-format-and-verification.md)); this ADR limits what it can say, not whether it is true.

A clock that steps back stalls the engine instead of corrupting the log's order. That is intended, and the error says why. It makes a correct clock an operational requirement.

An existing log whose events were written with broker-origin consequences of a gate answer, before this ADR, would fail replay. No release has written one: the log format and events are new in this milestone.
