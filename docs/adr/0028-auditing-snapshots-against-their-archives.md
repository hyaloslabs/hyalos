# ADR-0028: Auditing snapshots against their archives

**Status:** Proposed

## Context

[ADR-0026](0026-compacting-the-log-by-signed-snapshot.md) replaces compacted events with a snapshot of the state they led to, signed by the engine. `hyalos log verify` shows that a snapshot is the engine's and has not been altered. It does not show that the state in it is what the events replay to: the engine signed whatever state it held. The engine compares its state with a replay before and after it writes a snapshot, but that guards against ordinary mistakes, not against a build that is buggy or compromised, and an operator who prunes an archive ([ADR-0027](0027-recording-the-pruning-of-an-archive.md)) can no longer check anything afterwards. ADR-0026 left the command that does this open.

This ADR adds to ADR-0026 and changes nothing it decided.

## Decision

**`hyalos log audit` replays the history and compares.** Given the log and the archives (`--archive <file>`, any number), it checks every file as `verify --archive` does, takes the events from the archives and the log, and replays them from event 1. At each snapshot it compares the replayed state with the state the snapshot carries, and stops at the first that differs. It names the snapshot by the event it stands for and the first field that differs, with both values, for example `state.tasks[id 1].completed_at: the replay has 1005, the snapshot has null`. Fields are compared in alphabetical order and lists by position, naming an entry by its id, so the answer is the same every time. The events after the last snapshot are replayed too, so the whole history is accounted for.

**An audit that could not look is not a pass.** Only the run of events that starts at 1 and has no gap can be replayed. A snapshot beyond that run is not compared, and the output says which events no archive given holds and that the snapshot after them, and any later one, rests on the engine's signature alone. The exit status separates the three outcomes: **0** only if every snapshot was compared and matched (or the log was never compacted and its events replay); **1** if a snapshot differs, the events do not replay, or a file does not check out; **3** if the archives given leave a snapshot uncompared. A script cannot mistake a partial audit for a clean one.

**It reads and needs the public key, like `verify`.** It changes no file and takes no secret, so an auditor who holds the archives and the public key, and not the engine's key, can run it. It honours `--expect-head`.

**It compares what replay gives, not what the engine kept.** The replay uses the same fold the engine uses (`State::apply`), under the current event schema with older events upcast ([ADR-0023](0023-event-schema-versions-and-upcasting.md)), and the same image it writes into snapshots, so a state means the same thing on both sides. A snapshot whose state cannot be read, or says it ends at a different event than the snapshot stands for, is reported as unreadable, not skipped.

## Consequences

Once the archives are kept off the engine host, an operator can show that every snapshot the engine signed is what its events give, and a snapshot over a state the engine invented, signed with the real key, is found. This is the only check that does; `verify` cannot.

The audit is only as complete as the archives it is given. After an archive is pruned, the snapshots whose history it held can no longer be audited from this machine, and the command says so; the archive kept elsewhere is what makes it possible again. A fuller account of what was pruned is in the log ([ADR-0027](0027-recording-the-pruning-of-an-archive.md)).

It reads every event of the history into memory to replay it, so its cost grows with the history. That suits an occasional audit, not a check on every start.

It compares state after the schema's upcast steps. A bug in a step is not found by this, since both sides would apply it.
