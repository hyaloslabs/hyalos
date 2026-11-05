# ADR-0054: The content gate on what a push carries

**Status:** Proposed

## Context

[ADR-0004](0004-one-signed-manifest-per-job.md) says the broker content-gates a tainted job's outbound text. [ADR-0008](0008-cedar-for-policy.md) keeps byte-level gates in code. [ADR-0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md) built the gate for a pull request's title and body: it refuses a credential the broker holds (`content.credential`) and text that hides from its reader (`content.hidden_text`). It left one question open for S3.14b (#183): whether push contents are inflated and scanned.

A job's other outbound text is what it pushes through the git proxy ([ADR-0046](0046-the-git-proxy.md)): commit messages, file names and file contents. The proxy reads a push's ref updates but passes its pack on unopened. A pack holds objects compressed with zlib, and many are deltas against other objects. So a job told by an injected issue to commit the forge token, or a provider key, would publish it to the branch where anyone with access to the repository reads it.

Git normally sends a thin pack. In a thin pack, an edit to a file the forge already has is a delta against the forge's copy. The new text is then only the delta's inserted bytes, and nothing in the pack shows the whole file. A gate that checks only what it can rebuild would miss a credential built by copying part of it from the base.

[ADR-0020](0020-gate-answers-timeouts-and-holds.md) says a gate that cannot answer holds the job, and only a human can release it. The broker reports an unprompted `gate` to the engine ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md)), which records an `unknown` answer through `rule_gate`. Before this ADR no broker gate sent one.

## Decision

**Every push is content-gated before it reaches the forge** (`hyalos_broker::pack`, called from the git proxy once the ref rules of ADR-0046 have passed). As for pull requests, the gate applies to every job, not only a tainted one: every actor is tainted, and refusing what nobody should publish costs an untainted job nothing.

**The broker asks for packs it can read whole.** It adds the `no-thin` capability to the forge's push advertisement (`git::no_thin`). A git client must not send a thin pack to a server that advertises it, so every delta's base is in the same pack. The broker reads the whole advertisement (up to 64 MiB) to rewrite it. One without a capability list is a 502 (`git.bad_advertisement`).

**The pack is inflated and every delta resolved.**
- Each object is inflated with `miniz_oxide`, a pure-Rust zlib that forbids unsafe code. It must come to exactly the size its header states, with a valid checksum.
- An offset delta is applied to its base, which must be an object earlier in the pack. The delta must state its base's size, copy only from inside it, and build exactly the size it says.
- After the last object only the pack's checksum may remain. The forge verifies it.
- All objects together, deltas included, may come to 256 MiB once inflated (`MAX_INFLATED`). A push itself is held to 64 MiB, so this is a cap on expansion.
- A delta against an object named by id (a ref delta) is not resolved. A git client asked for no thin pack sends offset deltas to a server that offers them, which GitHub and `git http-backend` both do.

**What is checked in each object, once whole:**
- **Every object** (blob, tree, commit and tag) is refused if it holds a credential the broker holds: the job's forge token or a provider key, as raw bytes, 12 bytes or more. Reason: `content.credential`.
- **Commit and tag objects**, which a reviewer reads as text, must be UTF-8 and pass ADR-0048's check for hidden text, over the whole object (message, author, committer and the rest). Reason: `content.hidden_text`.
- **Names in a tree** must be UTF-8 and free of ADR-0048's hiding characters. Reason: `content.hidden_text`.
- **File contents are not checked for hidden text.** Source files and binaries hold control characters, zero-width joiners and bidirectional marks for good reasons, so such a check would refuse ordinary work. GitHub marks hidden bidirectional text in a diff, and review is the defence there.

**Push options are outbound text too.** When a push asks for `push-options`, the options between its commands and its pack are read and each must pass ADR-0048's check.

**What the broker cannot read, it does not pass, and the job holds.** A pack the broker cannot read whole is the content gate's `unknown`, never an allow and not a deny. That covers a pack that is corrupt (`content.pack_unreadable`), too large to inflate (`content.pack_too_large`), or holds a ref delta (`content.ref_delta`). The harness gets a 403. The engine records the `unknown` through `rule_gate` with irreversible stakes, so the job ends held and only a human can release it (ADR-0020, ADR-0025). `hyalos run` then stops the sandbox, ends the job in the broker and revokes the token, as at any end ([ADR-0050](0050-running-one-job-from-the-command-line.md)).

**A refusal is reported** as ADR-0048's are: a 403 to the harness, and the content gate's `deny` (`Gate::Content`) with a SHA-256 of what was refused. That is the whole object as the pack builds it, or the push option. An `unknown` report carries the SHA-256 of the pack. Both share the content gate's per-job cap (`content.too_many_refusals`). Nothing of a refused push reaches the forge.

## Consequences

A credential the broker holds cannot leave in a commit, a file, a file name or a push option, whether it is sent whole or built by a delta. A commit message cannot hide text from the person reviewing it. A pack built to defeat the reader holds the job for a person instead of slipping past.

The cost is that every push is read in full: up to 256 MiB is inflated, resolved and held in memory while the push is checked. The broker checks one push at a time, so it never holds more than one such inflation, while other pushes wait with their bodies (64 MiB each, as before). The cap fails closed: a push that inflates past it holds the job rather than going through.

The gate is a byte check, and it cannot catch:
- a credential the job has encoded, split across files or paraphrased;
- a credential the broker does not hold, such as one already in the repository;
- hidden or misleading text in file contents;
- data that is not a credential but should not leave, which is #172's question (data flows).

Tests:
- `core/broker/src/pack.rs` builds packs by hand. They cover each object type, a credential built by a delta from its base, hidden text in a message, a tag and a tree name, and each kind of pack that is unchecked.
- `core/broker/tests/git.rs` pushes with the real git client through a job's socket to `git http-backend`. The pushes are a file holding the token, an edit adding it to a file large enough that git sends deltas, a commit message with a bidirectional override, and a push option holding the token. Each is refused, reported with its digest, and never reaches the server. An ordinary edit of the same file passes. When this was built, the test was run once with `no-thin` turned off. That edit then arrived as a ref delta and the test failed, which shows the capability is what makes the pack whole. A raw push of a pack that is not one is the gate's `unknown`.
- `core/hyalos/tests/run_job.rs` runs that push from a sandbox under `hyalos run`. It shows the job ends held with the content gate's hold, the sandbox is stopped, and the token is revoked.

Not here:
- Comments, which have no route yet.
- GitLab.
- Checking file contents for hidden text by file type.
