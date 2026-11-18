# ADR-0112: Learnings queried over the event log

**Status:** Accepted

## Context

S9.1 (#57), the first story of E9 (#56): Hyalos should get better with use without creating a new way in. [ADR-0003](0003-event-log-is-the-database.md) makes the log the one mechanism that audit, replay and learnings share, and says the driving agent reads learnings only through the MCP surface ([ADR-0010](0010-mcp-primary-interface-two-principals.md)), never the database. Its acceptance: learnings come back with their provenance and their source events.

What the log can teach is already in it, as typed values: how each job ended and why, what each gate refused or could not decide, how holds and held tasks were decided, and the typed values each job returned. None of it is text from the forge or a model ([ADR-0003](0003-event-log-is-the-database.md)). A learning that pointed at no events could not be checked, and one that dropped where its values came from would carry a model's choice as if it were Hyalos's own record.

## Decision

**A learning is a lesson, its provenance, and the events it was learned from.** `hyalos_engine::learnings(path, key)` checks the log against the engine's public key, reads its events with `read_verified`, and folds them with `learn`, a pure function. A `Lesson` is one of:
- `JobEnded { outcome, reason }`: jobs ended this way, for this reason;
- `GateAnswered { gate, answer, reason }`: a gate refused or could not decide (allowing is not a lesson);
- `HoldResolved { by, reason }`, `HoldRefused { reason }` and `TaskCancelled { by, reason }`: how holds and held tasks were decided;
- `Values { job, values }`: the typed values one job returned.

Events with the same lesson are one `Learning`: its `count`, its first and last time, and its latest `MAX_SOURCES` (20) events, each by sequence number, hash in the chain and origin (engine or broker, [ADR-0024](0024-event-origins-and-time-order.md)). A reader can check any of them with `hyalos log verify`. Learnings come most recent first.

**Provenance is the ADR-0005 class of what the learning holds.** Outcomes, gates, answers, deciders and reason codes are Hyalos's own records from closed sets: `operator`. A job's values were chosen by a model, and the log does not say whether it had read tainted input, so `learn` gives them `model_from_tainted`. The MCP server knows more for a run it started: when the values are the run's result, it names them by the workflow and gives them the run's provenance, as `job_result` does ([ADR-0069](0069-jobs-the-driving-agent-starts-as-mcp-tasks.md)). Nothing ever makes a learning more trusted than that: unknown is tainted.

**An unreadable event is an error.** `learn` refuses rather than skip an event this build cannot decode, since a learning that left it out would say less happened than did. Events compacted into a snapshot ([ADR-0026](0026-compacting-the-log-by-signed-snapshot.md)) are not learned from, because a snapshot keeps state and not events: the answer's `from_seq` says where the learnings start.

**The driving agent asks with `list_learnings`, and `hyalos learnings` makes the same call.** The tool takes `limit` (1 to 100, default 20) and nothing else, reads the log and writes nothing. Its structured content is `total`, `from_seq` and `learnings`, each with its lesson's typed fields, `provenance`, `untrusted`, `count`, `first_at_ms`, `last_at_ms` and `sources`. Its text gives Hyalos's records as sentences and every untrusted learning only inside the `untrusted-data` element after the notice ([ADR-0072](0072-marking-untrusted-data-in-results.md)). As [ADR-0075](0075-the-thin-cli-for-the-agents-tools.md) requires of every tool, `tools_cmd::COMMANDS` gains `("learnings", "list_learnings")`, and the test that holds the server's tools to that table and to no decision covers it.

No event, wire message, manifest field or state is added: learnings are a projection read on demand.

## Consequences

The driving agent can ask what went wrong before and why, and check each answer against the signed log, without a new store or a new way to write. Later stories of E9 build on the same fold: a skill drafted from a successful run starts from the run's learned values, tainted.

A learning is only as good as the records: the broker's events are what the broker says ([ADR-0003](0003-event-log-is-the-database.md)), which `origin` makes visible. Learnings reach back only to the latest snapshot. Each call reads the whole live log, which is fine at the sizes compaction keeps it to, and would need an index if it were not.
