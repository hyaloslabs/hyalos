# ADR-0022: The engine's keys: generation, storage, backup and loss

**Status:** Accepted on November 2, 2026. The maintainer accepted the recommendations as written, including that v1 has no key rotation (a lost or leaked key ends that log and its state) and that the manifest key is ephemeral.

## Context

The engine signs two kinds of thing: every event in the log ([ADR-0018](0018-event-log-format-and-verification.md)) and one manifest per job ([ADR-0004](0004-one-signed-manifest-per-job.md)). [ADR-0002](0002-one-binary-two-processes-sandboxes.md) says the signing key lives only in the engine. Nothing yet says where a key comes from, where it lives, what a backup carries, or what happens when it is lost. The deterministic crates, which include the engine ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)), may not use operating-system entropy, so a key cannot be generated inside them. [ADR-0021](0021-backup-and-restore-of-the-event-log.md) leaves the key out of a backup on purpose, and a log whose chain is bound to one key ([ADR-0018](0018-event-log-format-and-verification.md)) cannot simply change keys.

Several keys exist and must not be confused:

| Key | Held by | Decided |
|---|---|---|
| Log signing key | the engine | here |
| Manifest signing key | the engine | here |
| Forge credential (GitHub App private key) | the engine | [ADR-0004](0004-one-signed-manifest-per-job.md); a credential, not decided here |
| Provider API keys | the broker | [ADR-0002](0002-one-binary-two-processes-sandboxes.md) |
| Operator approval key | the human | [ADR-0012](0012-humans-approve-only-irreversible-actions.md); kept apart, below |

## Decision

**Two engine keys, independent.** The log key lives as long as the log, because its public half is what anyone verifies history with, so it is rarely changed and everything depends on it. A manifest key only has to outlive one job. Giving them one key would tie the cheap thing to the expensive one. They share no material, and signatures use different tags.

**The log signing key** is an Ed25519 key generated once, from operating-system entropy (`getrandom`), by the `hyalos` binary, never by a deterministic crate. The binary hands the engine 32 bytes. It is stored in the config directory as `engine.key`: one line, `hyalos-engine-key-v1:` followed by 64 hexadecimal characters, mode 0600, in a directory with mode 0700. The prefix stops it being mistaken for another file, and hex lets an operator keep a copy in a password manager. It is written to a temporary file created exclusively, flushed, and renamed, so there is never a half-written key. The public key is then derived and written as hex to `engine.pub` with mode 0444 ([ADR-0018](0018-event-log-format-and-verification.md)). `engine.pub` is always derivable: if it is missing it is rewritten from the key, and if it exists and disagrees with the key the engine refuses to start.

**When the engine generates a key, and when it refuses.** It generates one only when it finds neither a key nor a log. A log with no key is a lost key, and the engine refuses to start and says so, rather than quietly beginning a log nobody can connect to the old one. A key with no log is fine. A key that does not match the log's first event is refused by the log layer today. The engine never continues a log with a different key.

**On load, permissions are checked.** The engine refuses a key file or directory that group or others can access, a key that is a symlink, and anything that is not a regular file. This is a stricter relative of the rule `hyalos log verify` applies to `engine.pub` ([ADR-0018](0018-event-log-format-and-verification.md)): the public key must not be writable by others, and the private key must not be readable or writable by them.

**The manifest signing key is ephemeral.** The engine generates it at each start, holds it in memory only, and gives its public half to the broker it launches over their private channel. It is never written, backed up or restored. A manifest therefore cannot outlive the engine process that signed it, which adds to the replay protection in [ADR-0004](0004-one-signed-manifest-per-job.md). The job lifecycle story decides what happens to a job whose manifest was signed before a restart.

**Storage is an owner-only file in v1.** The OS keyring and hardware-backed storage are not used. Both need components in the trusted core that the line budget and the dependency policy resist (a desktop bus client, vendor SDKs), neither exists on a headless server, and on Linux a keyring unlocked for a session is readable by any program running as that user, which is the same exposure as a 0600 file. The engine loads and stores the key through one place so a later decision can replace it.

**Backup is the operator's, separate from the log's.** `hyalos log backup` does not carry the key ([ADR-0021](0021-backup-and-restore-of-the-event-log.md)). The operator copies `engine.key` to storage that a compromise of the host cannot reach, such as a password manager or offline media. On first start the engine prints the public key and says to do this. To move to another machine: restore the log with `hyalos log restore`, place `engine.key` in the new config directory with mode 0600, and start the engine, which derives `engine.pub`, checks it against the log, and carries on appending. A restored log without its key still verifies from the public key, but cannot be extended.

**Rotation is not supported in v1.** A chain is bound to one key, and there is no handover event. The consequences are stated rather than hidden:

- **Key lost.** The old log stays verifiable forever from its public key, so the operator should keep a copy of it. The engine will not continue it. The operator can move `events.db` aside, as an archive verifiable with the old public key, and start a new log under a new key. Tasks, jobs and holds do not carry across.
- **Key compromised.** Whoever holds it can forge history from any point in time and the log cannot show which. The operator treats events from the earliest possible exposure as untrusted, archives the log, and starts a new one.

A rotation that links the old log to the new one is a later decision. The likely route is the signed snapshot of S1.7 (#94, [ADR-0003](0003-event-log-is-the-database.md)): the old key signs a snapshot and the new log starts anchored on it. It needs a format change and its own ADR.

**The key never leaves the engine's memory except to its file.** Key types print no bytes in `Debug` or `Display`. The key is held in a type that zeroes it when dropped. Events are typed and cannot carry it ([ADR-0003](0003-event-log-is-the-database.md)). A test scans the log file, standard output, standard error and a panic's output for the key, raw and as hex. Suppressing core dumps from inside the process needs calls the workspace's ban on `unsafe` forbids, so v1 documents disabling core dumps in the service unit (`LimitCORE=0`) and records a safe wrapper as follow-up work.

**The operator approval key is a different thing.** Its private half is created and kept by the operator's approval path ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)), in a different file in a different directory, and is never loaded by the engine. Its signatures use their own tag. The engine holds only the public keys the operator registered, and registering or changing one is an irreversible-tier action. A compromised driving agent holds neither key. The approvals story decides the rest.

**Dependencies.** `getrandom` is added to the `hyalos` binary crate only, with a license and advisory review in `cargo deny`. The deterministic crates gain nothing.

## Consequences

A fresh install needs no setup: the first start creates the directories, the key and `engine.pub`, and `hyalos log verify` then works with no arguments. A lost key is a documented, bounded event, not an unexplained failure, and it cannot lead to a log that quietly changes keys.

The cost of no rotation is that losing or leaking the key ends a log, and with it the engine's state. That is acceptable while Hyalos is young and logs are short, and it is the first thing to revisit if logs become long-lived. The cost of a file is that code running as the operator can read the key; the sandbox and the broker cannot, and that is the boundary the key needs to hold ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)).

Operators must back up one small file themselves. The first-start notice and the documentation say so, and a restored log without it is verifiable but frozen, which is the failure the operator would otherwise find out about late.

Because manifest keys die with the engine, nothing about manifests needs backing up or rotating, and the broker never holds a key that outlives its parent.
