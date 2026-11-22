# ADR-008: A session in the same second as the intent's signed time is refused

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (cold-read); accepted by the developer on 2026-11-21, after the cold read passed

## Context

S-03.3 refuses an answer unless its session time falls between the times the public log signed for the run's intent link and result link, and S-03.3.e2 refuses a session earlier than the intent. The session time and the log's signed times are whole seconds. Within one second, a session made before the intent was logged cannot be told from one made after it. Cold-read rounds 7, 8 and 10 each touched this edge.

## Options

1. **Both edges inside the window.** Rejected: a session made up to a second before the run was declared would pass, which S-03.3.e2 exists to refuse, and no limit in S-04.5 would disclose it.
2. **Both edges inside, with the one-second residual named as a fifth limit.** Rejected: the residual can be removed at the cost of a short wait, so naming it is weaker than closing it.
3. **The edge at the intent is strict, and the monitor waits.** Chosen.

## Decision

A session must be later than the intent's signed time, in whole seconds, and no later than the result's. A session in the same second as the intent's signed time is refused. After logging the intent, the monitor waits until its own clock is two seconds past the intent's signed time before the first model call.

## Consequences

- Every run takes up to two seconds longer.
- A notary whose clock runs more than a second behind the public log's has its sessions refused. That is the fail-closed result, and the customer must keep the notary's clock set.
- This is stricter than the literal "falls between" of S-03.3. The criterion's text is unchanged, and this decision is its reading.
