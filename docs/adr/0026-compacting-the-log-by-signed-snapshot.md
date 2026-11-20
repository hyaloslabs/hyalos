# ADR-0026: Compacting the log by signed snapshot

**Status:** Accepted; its rule that the engine names every field is superseded for the plain records by [ADR-0125](0125-the-state-holds-the-snapshot-s-record-types.md)

## Context

[ADR-0003](0003-event-log-is-the-database.md) promises that log growth is handled by compaction: a signed snapshot anchors the hash chain so `hyalos log verify` still works across it, compacted segments are archived and can be exported, and pruning an archive is an explicit operator action. It leaves open what a snapshot is, what it commits to, and what verification can and cannot say afterwards. [ADR-0018](0018-event-log-format-and-verification.md) and [ADR-0021](0021-backup-and-restore-of-the-event-log.md) both leave this to S1.7.

The pressure is real: the engine rebuilds its state by replaying the log ([ADR-0019](0019-typed-events-and-in-memory-projections.md)), so start-up time grows with every event ever written. The constraints are as firm: the log is never rewritten ([ADR-0023](0023-event-schema-versions-and-upcasting.md)), the hash covers the stored bytes ([ADR-0018](0018-event-log-format-and-verification.md)), and nothing the audit trail holds may be lost by accident.

## Decision

**Compaction never rewrites an event.** The events that follow a snapshot keep the hashes and signatures they were written with, and the first of them links to the snapshot's head hash, which is the hash of the last event that was removed. So every hash and every head is the same as in a log that was never compacted, and a head an operator recorded earlier (`--expect-head`) still matches. A test feeds a compacted log and an uncompacted twin the same events and requires equal heads at every step.

**A snapshot is a signed record in its own table, `anchors`.** It holds the sequence number of the last event it covers, when it was made, that event's hash, the state version, the state itself, a digest of the state, the hash of the previous snapshot, its own hash and the engine's signature on that hash. The hash and the signature use their own tags, so a snapshot cannot be replayed as an event or the other way round. It is not an event: the event set is closed ([ADR-0019](0019-typed-events-and-in-memory-projections.md)) and a snapshot stands in for events instead of describing one. Snapshots are never updated or deleted, and each commits to the one before it, the first to the log's key, so removing one breaks the chain.

**What a snapshot carries is a versioned image of the state.** `StateImage` (in `hyalos-types`) mirrors the engine's tasks, jobs and holds field for field, as JSON. The log signs the bytes and never reads them. The engine's conversion names every field, so adding one to the state stops compiling until the image carries it. The image has its own version. When it changes, the version is bumped and a step brings the old one up, as for events; a version this build does not know is refused, never skipped. Version 1 is pinned in `core/types/tests/snapshot.rs`.

**Archive first, then replace.** `Log::archive` writes a verified copy of the whole log to a new file, with the mechanism of backup ([ADR-0021](0021-backup-and-restore-of-the-event-log.md)): taken by SQLite, checked against the key before it exists under its name, never overwriting. The archive is made read-only, and it includes the earlier snapshots, so it verifies on its own. `Log::compact` is then given the head that was archived and, in one transaction, refuses if the log has moved, writes the snapshot, removes the events, and sets the format version. The guard against deleting events is lifted for that one statement and put back inside the same transaction, so a failure restores both. Compaction takes everything up to the head; there is no "keep the last N live".

**The engine only snapshots a state it has checked.** Before writing, it replays the previous snapshot and the events it is about to remove and requires exactly its live state. After writing, it verifies the log and rebuilds the state from the new snapshot and requires the live state again. A mismatch is an error, and after the write it stops the engine; the archive holds every event, so nothing is lost.

**Format version.** A log is written at format version 1 and the first compaction sets 2, so a build from before snapshots refuses a compacted log instead of misreading it.

**Verification crosses the snapshot.** The verifier checks the snapshots first (digest, hash, signature, chain, key) and starts the event chain at the latest one. `hyalos log verify` alone proves the events since the snapshot and that the snapshot is the engine's. `--archive <file>` (any number) also checks each archive on its own and that it ends exactly where a snapshot of this log stands, with that snapshot's hash, so another log's archive or an altered one is refused. The output says whether the whole history from event 1 was covered. The history before the latest snapshot otherwise rests on the engine's signature on it.

**Starting from a snapshot.** `Engine::open` verifies the file, loads the state of the latest snapshot and replays only the events after it. Time and sequence order continue across the snapshot: the stamp of the last event is part of the state.

**Pruning is an explicit operator action.** Hyalos never deletes an archive. `hyalos log prune <archive> --yes` does, after checking that the archive verifies and ends at a snapshot of this log, and it asks for `--yes` because it is the only command that removes history from the machine. It is the operator's command line. The engine's API has no call that prunes, so nothing a driving agent can reach does.

**Compaction is run by the operator with the engine stopped.** `hyalos log compact` opens the log with the engine's own key, since it signs. If the log changes between the archive and the transaction, it refuses and changes nothing.

## Consequences

Start-up time is bounded by the events since the last snapshot, and the file stops growing without limit once archives are moved off the machine. Audit is not weakened while the archives exist: verifying with them covers every event from the first, and the snapshots are checked against them.

**After an archive is pruned, that part of history rests on the engine's signature on the snapshot.** A compromised or buggy engine could have signed a state that its events do not give. Only the archive can show that, so an operator should keep archives somewhere the engine host cannot write before pruning them.

**A snapshot's state is a cache signed by the engine, not independent proof.** `verify` checks that it is the engine's and unaltered, not that it is what the archive replays to. The engine checks that itself at compaction (above) and tests compare it, but a command that replays the archives and compares is not part of this decision (S1.15).

**Pruning leaves no record in the log.** The command's output and the missing archive are all there is. Recording it needs a new event kind and so a schema version ([ADR-0023](0023-event-schema-versions-and-upcasting.md)), and is not part of this decision (S1.14).

Older builds refuse a compacted log, by design. Every compaction adds a snapshot holding the whole state at that point, and older snapshots stay as evidence, so the file grows by one state per compaction. Key rotation is still not supported ([ADR-0022](0022-engine-keys-generation-storage-backup-and-loss.md)); a chain of snapshots belongs to one key as the events do.

A backup of a compacted log is a backup of the live file, which holds the snapshots and the events since; the archives are separate files and are the operator's to keep.
