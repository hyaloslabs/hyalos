# ADR-0021: Backup and restore of the event log

**Status:** Accepted

## Context

[ADR-0003](0003-event-log-is-the-database.md) says backup is snapshot plus replay, and story S1.3 asks that a restore onto a fresh machine reproduces state byte for byte. [ADR-0019](0019-typed-events-and-in-memory-projections.md) keeps projections in memory and rebuilds them by replay, so the log is the only thing to back up. Copying the database file is the obvious way and a wrong one: the log runs in write-ahead mode, so the newest events can sit in a `-wal` file beside it, and a copy of `events.db` alone silently loses them. A backup is also worthless if it is not checked, and a restore that overwrites a log is a way to destroy one.

## Decision

**A backup is a consistent copy made by SQLite.** `VACUUM INTO` reads one snapshot of the database, including events still in the write-ahead log, while the engine keeps appending. The copy is a single self-contained file in rollback-journal mode with no `-wal` or `-shm` beside it, readable by its owner only. This is a copy of the log, not the signed snapshot of compaction in S1.7; the word means the same thing only in that both anchor a restore.

**A copy is verified before it exists under its final name.** It is written to `<name>.partial`, created exclusively and owner-only, then checked exactly as `hyalos log verify` checks a log: every sequence number, link, hash and signature against the operator's public key, and the recorded head if the operator gives `--expect-head`. Only a copy that passes is flushed and renamed into place. One that fails is removed, the source is not touched, and the failure names the first bad event.

**Nothing is overwritten.** The destination must not exist, a leftover `.partial` from a crashed run is refused rather than reused or deleted, and a dangling symlink at the destination counts as something there.

**Restore is the same operation into the live location.** `hyalos log restore <backup>` creates the log at the data directory (or `--db`), making the directory owner-only if it is new. It does not replace an existing log; an operator who wants to must move the old one aside, which is deliberate. Backup and restore take the same key and head arguments as `verify`, with the same rule that the trust anchor comes from the operator, never from the file. On a fresh machine there is no `engine.pub` yet, so the operator supplies the original public key; the command says that the engine will also need the original signing key to add to the log, which is story S1.8 (#113).

**What "byte for byte" means.** Every stored column of every event is identical in the copy, including each hash and signature, and the chain ends at the same head. Replaying the copy gives a state equal to the original's, and the same next event appended to both gives the same hash and signature, which can only be true if the histories are identical. The SQLite file itself is not promised to be byte-identical, because page layout is not part of the audit trail.

## Consequences

A backup can be taken while the engine runs, and an operator can tell from the command's exit status whether the copy is good, so a backup is never a file that merely looks like one.

Tail truncation is still invisible to the file alone ([ADR-0018](0018-event-log-format-and-verification.md)): a backup taken from a log whose newest events were removed verifies as a valid shorter chain. Operators who record the head after significant events and pass `--expect-head` close that gap, for backup and for restore.

A backup holds exactly what the log holds, which is digests and references and no secrets ([ADR-0003](0003-event-log-is-the-database.md)), but it is not encrypted. Its owner-only mode protects it on the machine; keeping it elsewhere safely is the operator's job. The signing key is not part of a backup, and its storage and recovery are decided with S1.8.

This ADR does not schedule backups, prune them, or export compacted segments (S1.7).
