# Compacting the log: archive, snapshot, prune

How to keep the event log small without losing the audit trail. The reasons are in [ADR-0026](../adr/0026-compacting-the-log-by-signed-snapshot.md) and, for recording a prune, [ADR-0027](../adr/0027-recording-the-pruning-of-an-archive.md); backups are in [keys-and-recovery.md](keys-and-recovery.md) and [ADR-0021](../adr/0021-backup-and-restore-of-the-event-log.md).

Compaction moves every event into an archive file and leaves, in the log, a snapshot of the state they led to, signed with the engine's key. **Nothing is deleted and no hash changes**: a head you recorded before still matches, and events added afterwards carry on from the snapshot.

## Compact

Stop the engine, then:

```
hyalos log compact
```

It uses `engine.key` from the config directory (`--key-file` for another) and writes the archive to an `archive` directory beside the log (`--archive-dir` for another), named for the events it holds, for example `events-0000000001-0000004210.db`. The archive is read-only and is checked before it exists under that name. It never overwrites a file; if one is in the way, move it aside. If the log changes while this runs, it refuses and changes nothing.

It prints the head the snapshot stands for. That is the same `seq:hash` the log had, so you can compare it with the one you recorded.

## Check

```
hyalos log verify
```

now says `ok: 0 events after the snapshot at 4210:9f2c…`. That proves the snapshot is the engine's and the events since it are intact. The history *before* the snapshot then rests on the engine's signature on it. To check it too, give the archives:

```
hyalos log verify --archive archive/events-0000000001-0000004210.db
```

Give one `--archive` per file. Each is checked on its own and must end exactly where a snapshot of this log stands, so another log's archive, or one that was altered, is refused. The output says `history from event 1 verified` when the archives given cover everything, and says what is not covered when they do not.

## Keep the archives

An archive is the only independent copy of that part of the history. Before you rely on a pruned log, copy the archives somewhere the engine host cannot write, such as another machine or offline media, and check them there with `hyalos log verify --archive` against your copy of the public key. `hyalos log backup` copies the log file only; the archives are separate files.

## Prune

Stop the engine, then:

```
hyalos log prune archive/events-0000000001-0000004210.db --yes
```

This deletes that one archive, after checking that it verifies and belongs to this log. It is the only command that removes history from the machine, and the engine has no call that does the same, so only you can run it.

**It is recorded in the log.** Pruning signs a record, so it needs the engine's key (`engine.key` in the config directory, or `--key-file`) and the engine stopped, as `compact` does, and not just the public key. The record (an `archive.pruned` event) names the archive by the event it ended at and that event's hash, and is written **before** the file is deleted. If the command stops in between, the record is there and the file is too: run it again and it says `already recorded in the log` and removes the file, without a second record. If the key is wrong, or the record cannot be written, nothing is deleted.

`hyalos log verify` lists what was pruned, from the log and the public key alone:

```
pruned: the archive ending at event 4210 (9f2c…) was removed, recorded at event 4211, unix time 1767225600
```

The list survives later compactions: the next snapshot carries it. After a prune, the history that archive held rests on the engine's signature on the snapshot, as above.

**What the record is and is not.** It is the trace the supported command leaves. It does not stop anyone with access to the files from deleting an archive by hand, which leaves no record; an archive that is missing from where you keep them and is not in this list was removed some other way. Compare the directory you keep archives in with the list.

## Older versions

A log that has been compacted is refused by a build from before snapshots, with a message that its format is not supported. A log that records a prune is refused by a build from before schema version 3, with a message to upgrade. Upgrade first; do not try to open either with the old build.
