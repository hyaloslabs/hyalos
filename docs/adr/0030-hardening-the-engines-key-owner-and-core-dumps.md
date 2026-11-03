# ADR-0030: Hardening the engine's key: its owner, and core dumps

**Status:** Proposed

## Context

[ADR-0022](0022-engine-keys-generation-storage-backup-and-loss.md) says the engine refuses a key file or config directory that group or others can access, and records two things left for later: the check looks at modes only, and the workspace's ban on `unsafe` means the engine cannot switch core dumps off from inside itself, so v1 tells the operator to set `LimitCORE=0`. The review of the code that implemented ADR-0022 found the first gap. A key owned by another user with mode 0600 passes a mode check, and that user can read it, replace it or hand it to someone else.

This ADR adds to ADR-0022 and changes nothing it decided.

## Decision

**The key and the config directory must belong to the user running the engine.** On load, the engine compares the owner of `engine.key` and of the config directory with its effective user id and refuses on a difference, however private the mode. The message names both ids and says what to do: `chown` it if it is yours, and if you did not put it there treat it as compromised. The same check applies to every command that reads the key by path (`hyalos log compact`, `hyalos log prune`). It is not applied to `engine.pub`, which holds nothing secret and is checked by what it must match.

Effective, not real, user id: it is the identity the kernel uses to decide who may read the file. A key owned by the operator is refused when the engine runs as root through `sudo`; run it as the operator, or as the service user that owns the key.

**The user id comes from a safe wrapper.** The workspace forbids `unsafe`, and reading the user id needs a system call. The `hyalos` binary crate takes the `rustix` crate (with its `process` feature only) for it: a thin, widely used wrapper over the system calls that exposes them as safe functions. It is added to `hyalos` only, not to the deterministic crates ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)), and passes `cargo deny`. Reviewing a dependency is part of this decision, not an afterthought, because a crate here is code that runs with the key in memory.

## Consequences

A key another user placed, or kept after changing ownership, is now refused, and a directory shared between users is refused even when its mode is private.

A test cannot give a file to another user without being root, so the unit tests cover the check by naming a different user for the same files, and a test that really changes the owner runs only where the tests run as root.

A deployment that runs the engine under a different user from the one that made the key has to change the owner once. That is the intended friction.
