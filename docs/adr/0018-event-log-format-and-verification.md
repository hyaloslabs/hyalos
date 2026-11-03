# ADR-0018: Event log format and verification

**Status:** Proposed

## Context

[ADR-0003](0003-event-log-is-the-database.md) says every state change is an append-only, hash-chained, signed event in SQLite, that only the engine writes, and that `hyalos log verify` works. It does not say which algorithms, which bytes are hashed, or what the verifier trusts. A log format is hard to change once logs exist, and a verifier that trusts the wrong thing proves nothing, so these are decided before the first event is written.

## Decision

**Storage.** One SQLite file in WAL mode with `synchronous=FULL`, created readable by its owner only. The `events` table is strict: `seq` (from 1, no gaps), `at` (milliseconds since the Unix epoch), `origin` (`engine` or `broker`), `kind`, `schema_version`, `payload` (opaque bytes), `prev_hash`, `hash`, `signature`. Triggers refuse `UPDATE` and `DELETE`. They stop an engine bug from rewriting history; they do not stop someone who has the file, which is what the chain and signatures are for. The table layout carries a format version, and a build refuses a version it does not know.

**What is hashed.** SHA-256 over a fixed tag, then `seq`, `at`, `origin`, `kind`, `schema_version`, `payload` and `prev_hash`, with every number big-endian and every variable-length field preceded by its length. The hash covers the stored bytes, so the verifier recomputes it from the row and never re-serializes a payload, and no canonical form is needed. The hash of event 1's predecessor is SHA-256 of a tag and the log's public key, so a chain belongs to one key and fails at event 1 under any other.

**What is signed.** Ed25519 ([RFC 8032](https://www.rfc-editor.org/rfc/rfc8032), deterministic) over a second tag followed by the event hash. Verification is strict: it refuses malleable signatures and weak keys. The key exists only in the engine ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)); `hyalos-log` takes it as 32 bytes and cannot generate one, because that needs operating-system entropy, which a deterministic crate may not use ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)).

**The verifier trusts the operator, not the file.** `hyalos log verify --db <path> (--public-key <hex> | --public-key-file <path>) [--expect-head <seq>:<hash>]` opens the database read-only and checks, in order, for each event: the sequence number, the link to the previous hash, the recomputed hash and the signature. It stops at the first failure and names the event and the reason. The public key is never read from the database, because someone who can rewrite the file could replace it. Exit status is 0 for a verified log, 1 for a failure and 2 for a usage error.

**Origin.** The broker has no dependency on `hyalos-log` (checked by `cargo xtask layout`), so it cannot open the database or sign. The engine appends what the broker submits over the private channel with origin `broker`.

**What the log layer does not do.** It stores bytes. That events hold only typed values, digests and references ([ADR-0003](0003-event-log-is-the-database.md)) is enforced by the typed event definitions that come with the projections (S1.2), and tested by scanning a finished job's log for planted secrets once a job can run.

**Dependencies.** `rusqlite` with its bundled SQLite (the engine runs the version we tested), `ed25519-dalek` without its random-number feature, and `sha2`. They are the only external crates `hyalos-log` may use, by an entry in `cargo xtask`, and their licenses (BSD-3-Clause, Zlib, Unicode-3.0 on top of MIT and Apache-2.0) are allowed in `deny.toml`.

## Consequences

Any edit to a stored event, any removed or reordered event, and any forged event fails verification at the first affected event, and an attacker without the signing key cannot repair it by recomputing hashes.

Three things the chain cannot do, stated so nobody relies on them:

- **Removing events from the end is invisible to the file alone.** What remains is a valid chain. Only a head recorded elsewhere (`--expect-head`) exposes it, so operators should record the head after significant events. The signed snapshots of compaction ([ADR-0003](0003-event-log-is-the-database.md), S1.7) anchor it further.
- **Someone with both the file and the signing key can rewrite history.** The key protects against tampering by anyone without the engine's privileges, not against a compromised engine.
- **Key generation, storage, rotation and backup are not decided here.** A rotation would start a new chain segment and needs its own decision. Until then, a log has one key for its life.

Changing the hashed bytes changes every hash, so it needs a new format version and an ADR. The `rusqlite` dependency tree includes a hash table with a randomly seeded hasher inside its statement cache; its iteration order is not observable from our code.
