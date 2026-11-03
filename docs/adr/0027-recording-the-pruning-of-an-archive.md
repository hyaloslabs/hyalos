# ADR-0027: Recording the pruning of an archive

**Status:** Accepted

## Context

[ADR-0026](0026-compacting-the-log-by-signed-snapshot.md) makes `hyalos log prune` the one command that removes history from the machine, and says plainly that nothing in the log shows it happened: the command's output and a missing file are all there is. That is a gap in an audit trail whose point is that nothing is removed without a record. Recording it needs a new kind of event, and so a schema version ([ADR-0023](0023-event-schema-versions-and-upcasting.md)), which is why it was left out of that decision.

This ADR adds to ADR-0026 and changes nothing it decided.

## Decision

**A prune is an event: `archive.pruned`.** It holds `last_seq` and `last_hash`: the last event the archive held and that event's hash. That is also where the snapshot that stands for the archive stands, so it names the archive without a file name, which an operator can change. Only the engine may submit it ([ADR-0024](0024-event-origins-and-time-order.md)); a broker cannot say that history was removed.

**It is written before the file is deleted.** If the process stops between the two, the log says the archive was pruned and the file is still there; running the command again sees the record, writes none, and deletes the file. The other order could leave a deleted archive and nothing to say so, which is the case this exists to prevent.

**The engine only records an archive its own log made.** `Engine::record_prune` refuses unless a snapshot of this log stands at exactly that event with exactly that hash. The state refuses a second record for the same archive, and a record that names an event the log has not reached. So a record is about one of this log's archives, once.

**Pruning is now a signing act.** `hyalos log prune` takes the engine's key (`--key-file`, default `engine.key`) as `compact` does, and the engine stopped, instead of a public key. It still checks the archive first (it verifies, holds events, and ends where a snapshot of this log stands) and still needs `--yes`. If the key does not match the log, or the record cannot be written, nothing is deleted.

**The record is kept in the snapshot.** The state holds the list of pruned archives, so the next compaction carries it into its snapshot instead of moving it into an archive where it could be pruned in its turn. The state image goes to version 2 with the list; a version 1 image is read as having none ([ADR-0026](0026-compacting-the-log-by-signed-snapshot.md) keeps every version it ever wrote readable).

**`hyalos log verify` lists them.** From the latest snapshot and the events since, with the public key alone and without replaying the log, so it still works on a log this build could not rebuild a state from. It prints which archive (the event it ended at and its hash), the event that recorded it and when.

**Schema version 3.** The new kind is the only change; no earlier payload moves, so no step is needed. A build that reads up to version 2 refuses a log that holds one, with the usual advice to upgrade.

## Consequences

An operator, and anyone with the public key, can see which archives were removed and when, in the same signed chain as everything else, and the list survives compaction.

**The record is of what the command did, not a barrier.** Someone with the host's files can still delete an archive by hand, with no record, and someone with the engine's key can write a record for an archive they have not deleted. A missing archive that the log does not list as pruned is a sign of the first. `verify` cannot name one it was never given, because archives are optional; an operator who keeps the archive directory off the engine host compares it with the log. The point is that the supported path leaves a trace, so that a removal with no trace stands out.

Pruning can no longer be done with a public key alone. That is the point, and it matches `compact`: both change what the machine holds, so both are the engine's.

The list in the snapshot grows by one entry per prune and is never trimmed, as the snapshots are not.

Checking that a snapshot's state is what its archives replay to is still not done here (S1.15).
