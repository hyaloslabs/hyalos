# ADR-0070: The operator's signature on every human decision

**Status:** Proposed

## Context

ADR-0003 says an approval event carries the operator's signature as well as the engine's, and
ADR-0012 says that in v1 the operator approves with `hyalos approve <id>`, signed with a local key the
driving agent cannot use. Until now a human decision was a claim: whatever called
`Engine::decide_hold`, `cancel_task`, `approve_policy`, `release_handle`, `issue_agent` or
`revoke_agent` with `Decider::Human` was recorded as a human, signed by the engine's key alone. The
engine's key is on the machine the driving agent works on, so an agent that can read `engine.key`, or
a buggy or compromised engine, could write a decision that looks like the operator's, and nothing
could tell it apart. ADR-0068 gave the agent a bearer credential and said S5.4's approvals must refuse
it.

Three choices had to be made: what the operator's key is and how it is kept from the agent; which
decisions it signs; and what becomes of human decisions already in a log.

## Decision

**The key.** `hyalos operator init` makes an Ed25519 key, the operator's approval key, from the
operating system's entropy. Its seed is kept in `operator.key` in the config directory (0600, read
with the same owner and mode checks as `engine.key`), sealed with ChaCha20-Poly1305 under a key that
PBKDF2-HMAC-SHA256 (600,000 iterations, 16-byte salt) derives from a passphrase of at least 12
characters; the seal is bound to the format and the public key written beside it. Both primitives come
from `ring`, already in the `hyalos` crate. The public half is `operator.pub` (0444). A file asking for
fewer than 100,000 or more than 10,000,000 iterations is refused. The key is never replaced: `init`
refuses if either file exists. The passphrase is read from `/dev/tty` with echo off (rustix's
`termios`; a signal during the read puts echo back before the command exits), or as one line from stdin with `--passphrase-stdin` for a script the operator runs and for
tests; never from an argument, the environment or a file. An agent that can read every file still
cannot sign without the passphrase.

**What is signed.** Every event that records a decision gains a field `signed`: `none` (a decision
by policy, the verifier or a gate), `unrecorded` (a human decision written before this version), or
`{"operator": {operator, after, after_hash, signature}}`. Schema version 13 adds it to
`hold.resolved`, `task.cancelled`, `policy.approved`, `handle.released`, `agent.issued` and
`agent.revoked`; an older human decision reads as `unrecorded`, any other as `none`. The signature is
Ed25519 over the tag `hyalos.approval.signature`, the engine's public key (which names the log),
`after` (8 bytes big-endian) and `after_hash`, the last event the engine read before writing and its
hash, then the event's JSON with `signed` set to `none`, so it covers every other field. Types and
checks are `hyalos_types::approval` (`OperatorKey`, `OperatorPublic`, `OperatorApproval::is_by`,
`Signed`) and `Event::{decided_by, signed, with_signed, approval_bytes}`.

**Every human decision.** The engine signs only if it was given the key (`Engine::with_operator`);
without it a human decision is `EngineError::NoOperatorKey` and nothing is written. The key is unlocked
only by a command the operator runs: `hyalos approve <hold>` (new; `decide_hold` as a human, default
reason `approved.operator`), `hyalos cancel <task>` (new; `cancel_task`, `cancelled.operator`),
`hyalos agent issue|revoke`, `hyalos policy approve` and `hyalos run --release-issue`, which asks for
the passphrase before anything is written (`Setup.operator`). The MCP server has no tool that decides,
never opens `operator.key`, and the runs it starts cannot carry `--release-issue`. The engine signs
after catching up with other writers, so `after` is the event its check ran against.

**What the state checks.** The state knows no key, so it checks the signature's shape and place,
both on write and on replay: a human decision must be `operator` or `unrecorded`
(`HumanDecisionUnsigned`), any other decision `none` (`SignedButNotHuman`), and an approval's `after`
must be no later than the last event and no earlier than the last signed decision
(`ApprovalOutOfPlace`), so an approval cannot be written again later or held back past another one.
The state keeps that last signed decision (`StateImage` version 8, `last_approval`; older images read
as none). The engine never writes `unrecorded`.

**The version is no excuse.** An event's schema version is a column its writer chooses, so whoever has
the engine's key could write a human decision at version 12 and have it read as unsigned and old. The
state therefore records the first event written at version 13 or later (`signed_since`, kept in
`StateImage` version 8, so compaction keeps it), and `State::apply_written` refuses an `unrecorded`
decision written at version 13 or later, or after that event, on write and on replay. The engine, the
MCP server and minting therefore never act on one.

**What verify checks.** `hyalos log verify` runs `hyalos_engine::approvals` with the engine's public
key and the operator's, from `--operator-key`, `--operator-key-file` or `operator.pub` (refused if
writable by others), never from the log. Every human decision written at version 13 or later must be
signed by that key for this event in this log, with `after` before it and no earlier than the
previous signed decision, and `after_hash` equal to that event's hash when the event is in a file
given. A signed decision with no operator key to check it fails. Human decisions written before the
log reached version 13 pass and are listed as unsigned. The check runs over the archives given with
`--archive` as well as the live log, in sequence order, so compacting does not hide a decision from
it; history in no file given rests on the engine's signature on the snapshot it ends at (ADR-0026),
and verify says so. `StoredEvent` now carries each event's hash for this.

## Consequences

- An approval the engine's key alone wrote fails `hyalos log verify`, and replay refuses one that
  claims a human and carries no signature, at any version once the log has reached version 13. The driving agent's credential is a bearer secret and
  opens nothing: given as the passphrase it is refused, and no key made from it is the operator's.
- The operator types a passphrase for each decision, including issuing and revoking an agent's
  credential and approving a policy. Scripts and tests pass it on stdin. A lost passphrase or key file
  means no further human decision on this log until a new key is set up, which this ADR does not
  provide: the old signed decisions name the old key, so replacing it needs its own decision.
- `hyalos log verify` needs `operator.pub` once any decision is signed; an auditor on another machine
  passes `--operator-key` as they pass `--public-key`.
- Logs written before version 13 still open, verify and audit; their human decisions are listed as
  resting on the engine's key alone. Upgrade with no run in progress: an older build still running
  after this one has written could record a human decision the log then refuses on replay.
- What is signed is the event's payload, not its stamp: whoever has the engine's key could restamp a
  signed decision, within the rule that time never goes back (ADR-0024). The approval's `after` still
  binds it to its place in the log.
- The engine signs with whatever `operator.key` holds when the passphrase opens it, and verify checks
  against whatever `operator.pub` holds: someone who can replace both files before the first signed
  decision has replaced the operator. That access also reaches `engine.key`; keep a copy of
  `operator.pub` off the machine, as for `engine.pub`.
- Not built here: ADR-0012's merge approval bound to a pull request's head commit (no irreversible
  action mints yet), a hardware key or a second device for the operator, and rotating the operator's
  key.
