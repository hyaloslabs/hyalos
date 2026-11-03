# ADR-0020: Gate answers, timeouts and holds in the engine

**Status:** Proposed

## Context

[ADR-0009](0009-gates-answer-allow-deny-unknown.md) says every gate answers allow, deny or unknown, that unknown holds the task and opens a decision, that each answer records the gate, a digest of its inputs and a reason, and that fail-closed is enforced by the type. [ADR-0019](0019-typed-events-and-in-memory-projections.md) gave holds events and rules but nothing that produces one. Story S1.6 needs the interface gates answer through, what a timeout is, what is recorded, and a guarantee that no crash or bug can leave a task free to run after a gate failed to say yes.

## Decision

**The interface.** A gate is asked a `GateRequest` (which gate, and the digest of what it is asked) and returns a `Verdict` (allow, deny or unknown, with a reason) or a `Fault` (it timed out, or it was unavailable). The trait is in `hyalos-policy`; Cedar evaluation, the egress allowlist, the content gate and a verifier agent implement it as they arrive.

**One function decides what happens.** `ask` runs a gate against a deadline and returns an `Outcome`: `Proceed`, `Refuse` or `Hold`. It returns an outcome, not a `Result`, so an error, a timeout or a late answer has no path to `Proceed` and no caller can forget to handle one. Only an allow that arrives by the deadline proceeds. Everything else that is not an on-time deny holds:

- unknown holds, with hold reason `unknown`;
- a fault of timed-out, an answer that arrives after the deadline whatever it says, and a request whose deadline has already passed (the gate is not asked) all hold, with reason `timed_out`;
- unavailable holds, with reason `unknown` and the code `gate.unavailable`.

A late allow is discarded because the caller has already been told the gate did not answer. The deadline is a `Timestamp` the caller supplies, from the job's manifest or a per-gate limit, and the clock is the `Clock` trait, so a timeout is reproducible in simulation.

**What is recorded.** A new event, `gate.answered`, records the job, the gate, the digest of its inputs, the answer and a reason, for allow, deny and unknown alike. The reason is a `ReasonCode`: one to 64 characters of lowercase letters, digits, `.`, `_` and `-`, such as `egress.host_not_allowed`. A code cannot carry free text, so a reason cannot smuggle a secret, a prompt or model output into the log ([ADR-0003](0003-event-log-is-the-database.md)). A gate is answered only for a running job.

**Unknown is written with its hold, in one transaction.** The engine writes three events together: `gate.answered` (unknown), `job.ended` with outcome `held`, and `hold.opened` for the same task, gate and inputs. This follows the order [ADR-0019](0019-typed-events-and-in-memory-projections.md) requires (the job ends before the hold opens). The log gains an all-or-nothing multi-event append for this. The hold is the open decision: a timed-out gate leaves the task held with exactly one open hold. A deny refuses that one request and the job carries on.

**The state enforces it, so a bad log is refused.** After an unknown answer, the only event that may follow is the job's end as held, and after that only the hold for the same task, gate and inputs. Anything else is a rule violation. The engine will not write an unknown answer on its own, and a log that ends between the answer and the hold is refused when the engine opens, so a truncated or hand-written log cannot yield a task that looks free after a gate failed.

**Not decided here.** Resolving a hold (re-evaluating the gate, or a decision at the tier of the action, [ADR-0012](0012-humans-approve-only-irreversible-actions.md)) is a later story; the `hold.resolved` event exists and is checked. Holds that do not come from a gate answer are still legal events. Which origin may submit `gate.answered` is S1.9 (#117). Gate evaluation outside a running job is not covered.

## Consequences

Fail-closed is a property of the type and of replay, tested by a property over every combination of result and timing (only an on-time allow proceeds), by tests that reject logs where unknown did not hold, and by mutation of each rule.

The three-event write means a hold always has its answer on record, and an audit of a held task reads in order: the answer, the job's end, the hold.

`submit_all` checks a batch against a copy of the state, which costs a copy per hold. That is fine at the sizes v1 targets and goes away if projections stop being held whole (S1.7).

Reasons are codes, so a gate that wants to explain itself to a human at a decision needs a catalogue of codes with messages. That catalogue is the gate stories' work, and a code that is not in it is still recorded and still checked for shape.

`ask` is synchronous and deterministic, so it can refuse a late answer but cannot interrupt a gate that never returns. Whatever runs a real gate (the broker's request path, a verifier agent's call) must enforce a wall-clock limit itself and report `Fault::TimedOut` when it fires. The late-answer rule is the backstop for a gate that returns after the limit anyway, not a replacement for the limit.
