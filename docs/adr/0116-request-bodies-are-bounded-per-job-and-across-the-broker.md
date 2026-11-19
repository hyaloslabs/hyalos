# ADR-0116: Request bodies are bounded per job and across the broker

**Status:** Proposed

## Context

A job's socket takes up to 16 connections (ADR-0038). Each route reads a body up to its own cap: 64 MiB for git (ADR-0046), and 32 MiB for a model call or a plain-HTTP egress request. Before this decision, each body was allocated in full from its `content-length` before any byte arrived. A sandbox that declared large bodies and sent them slowly could hold about 1 GiB of broker memory per job, with no limit across jobs.

A push's pack is checked under one process-wide lock, so that the broker inflates at most one pack at a time (ADR-0054). The inflation budget charged each object only its inflated size, so a pack of millions of empty objects cost nothing against it. Reading such a pack still took seconds, and every other job's push waited behind it. The pre-read of the trusted core found both problems (#270, #336).

## Decision

**A body is held only as its bytes arrive, and every byte held counts against two limits.**
- The job's own limit, `PER_JOB`, is one git body plus one model call (96 MiB).
- The broker's limit, `IN_ALL`, is 512 MiB for all jobs together.

A body that would pass either limit is refused as busy with a 503: `git.busy`, `egress.busy` or `model.busy`. What a body held is given back when its call is answered. Reading stops when the job ends, and the engine ends a job when its deadline passes. `core/broker/src/bodies.rs` holds this.

**A pack's check is bounded in work, not just in bytes.**
- Each object costs at least `OBJECT_COST` (64 bytes) against `MAX_INFLATED`.
- A pack that says it holds more than `MAX_OBJECTS` (2^20) objects is not checked.
- The check stops once the job ends.

All three outcomes give the content gate's unknown, never allow (ADR-0009). The process-wide lock stays, because it is what bounds inflation memory to one pack, and the time it can be held is now bounded.

## Consequences

One job can hold at most `PER_JOB` of the broker's memory, so it can no longer exhaust the broker for the others. Up to five jobs at their own limits fit within `IN_ALL`. Beyond that, a body is refused as busy, not left waiting. A harness may retry a 503, and a git push fails and can be run again.

A push of more than about a million new objects is not checked, so the gate answers unknown and the job holds. A job's own work is far smaller than that. A legitimate import of a whole large history needs another route.
