# ADR-0003: The event log is the database

**Status:** Proposed

## Context

Today's hivrr keeps tasks, audit chains, checkpoints and config in separate stores, and bugs live in the seams.

## Decision

Every state change is an append-only, hash-chained, signed event in SQLite (WAL). Tasks, jobs, holds and config are projections. Schemas are versioned and old events are upcast on read.

## Consequences

Audit, replay, debugging, backup (snapshot plus replay) and learnings share one mechanism. Deterministic simulation becomes possible. Projections must be rebuildable, and log growth needs compaction by snapshot.
