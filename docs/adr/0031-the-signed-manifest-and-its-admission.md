# ADR-0031: The signed manifest and its admission

**Status:** Proposed

## Context

[ADR-0004](0004-one-signed-manifest-per-job.md) decides that the engine signs one manifest per job, that the broker enforces that document and nothing else, and that the broker refuses an unknown version, an expired manifest, a job that is not live and an invalid signature. It leaves the format, the signing, where the code lives and what "the credential used" is. [ADR-0022](0022-engine-keys-generation-storage-backup-and-loss.md) decides that the manifest key is ephemeral and that its signatures use their own tag. This ADR adds the pieces needed to build the first slice, S3.1, and changes nothing those two decided.

## Decision

**The document.** A manifest holds its schema version, the job id, the role (reader, planner, actor or deterministic step), whether it is tainted, the repo, the branch prefix, the actions, egress hosts, tools and data flows (each a set of names), a token budget, a deadline, and an optional approval (the head commit SHA it is bound to and the sequence number of the approval event). These are the fields ADR-0004 lists. The sets are sorted and without repeats, and the document is a struct, so a manifest is always the same bytes. The vocabulary of action and tool names is not decided here ([ADR-0004](0004-one-signed-manifest-per-job.md) leaves it to the story that defines it); until then they are plain names. Version 1 is the only version.

**What is signed.** Ed25519 over a tag (`hyalos.manifest.signature`), the schema version as four big-endian bytes, and the document's JSON bytes. The tag separates it from the signatures the engine makes for events and snapshots, so none can be sent as another. The version is signed, and it is also outside the document in the envelope the broker receives (`version`, `manifest` as hex, `signature` as hex), so the broker can refuse a version it does not know before it reads anything else, and an envelope cannot be relabelled with a version it was not signed under. A document whose own version differs from the envelope's is refused. The engine refuses to sign a document in a version it does not write.

**Where the code lives.** The document, both keys and the envelope are in `hyalos-types`, which takes `ed25519-dalek` for this (the crate and settings the log already uses; no randomness). The engine signs and the broker checks with the same definition, and neither depends on the other or on the log crate, so the layout rules of [ADR-0017](0017-workspace-layout-and-trusted-core-boundary.md) hold. A manifest key is made from a seed the binary draws from the operating system, as the log key is; the type holds it in memory and prints only the public half.

**Admission.** The broker's `Admission` checks, in this order and giving the first reason that fails: the version, the signature, that the payload is a manifest of that version; that the manifest's job is the credential's job; that the job is live; that the manifest's role is the credential's role; and that the deadline has not passed. A manifest at its deadline is still valid and one after it is not, as an answer exactly at a gate's deadline is on time ([ADR-0020](0020-gate-answers-timeouts-and-holds.md)). A refusal names the check and carries nothing from the document.

**The credential.** A credential is what the broker issued to a sandbox for one job at one role. The broker's record of it is authoritative, and the sandbox's claim is not: the manifest's role is compared with it, so a reader's credential cannot be used with an actor's manifest. How a credential is issued and presented is the model route's story (S3.2); here it is a job and a role.

**Liveness.** The broker learns which jobs are live from the engine, over the channel that story S3.11 builds. `Admission` asks a `LiveJobs` it is given, so it never guesses and a test can say which jobs are live.

**Not decided here.** Engine-side rules about what may be signed (that taint is derived from a step's inputs and the engine cannot sign an untainted manifest for a tainted one, S3.14), the broker's enforcement of each field (the later S3 stories), minting forge tokens, and the event that records a manifest. This slice is the document, its signature and the refusals.

## Consequences

A job is served only on a manifest the engine signed for it, for its role, while it is live and in time. A broker that is handed a forged, relabelled, stale, replayed or wrong-role manifest refuses it, and each case is tested down to a flipped bit.

`hyalos-types` now depends on `ed25519-dalek`. The manifest key is the engine's secret, so the engine and the binary must keep it in memory only ([ADR-0022](0022-engine-keys-generation-storage-backup-and-loss.md)); this ADR adds the type but does not start the engine, so nothing generates one yet.

Any change to the document is a new version with a new pinned form (the tests pin version 1's bytes and signature, and the signature was computed independently of the code).
