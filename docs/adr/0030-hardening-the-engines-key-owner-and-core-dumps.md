# ADR-0030: Hardening the engine's key: its owner, and core dumps

**Status:** Accepted

## Context

[ADR-0022](0022-engine-keys-generation-storage-backup-and-loss.md) says the engine refuses a key file or config directory that group or others can access, and records two things left for later: the check looks at modes only, and the workspace's ban on `unsafe` means the engine cannot switch core dumps off from inside itself, so v1 tells the operator to set `LimitCORE=0`. The review of the code that implemented ADR-0022 found the first gap. A key owned by another user with mode 0600 passes a mode check, and that user can read it, replace it or hand it to someone else. The second is ADR-0022's own: it records a safe wrapper as follow-up work.

This ADR adds to ADR-0022 and changes nothing it decided.

## Decision

**The key and the config directory must belong to the user running the engine.** On load, the engine compares the owner of `engine.key` and of the config directory with its effective user id and refuses on a difference, however private the mode. The message names both ids and says what to do: `chown` it if it is yours, and if you did not put it there treat it as compromised. The same check applies to every command that reads the key by path (`hyalos log compact`, `hyalos log prune`). It is not applied to `engine.pub`, which holds nothing secret and is checked by what it must match.

Effective, not real, user id: it is the identity the kernel uses to decide who may read the file. A key owned by the operator is refused when the engine runs as root through `sudo`; run it as the operator, or as the service user that owns the key.

**The process switches core dumps off for itself, at the start of every command and before any key is read.** It sets the core file size limit to zero, soft and hard, so nothing in the process can raise it, and on Linux it also marks itself not dumpable (`prctl(PR_SET_DUMPABLE, 0)`). Both are needed. The size limit stops a dump to a file, but a `core_pattern` that pipes to a crash handler (systemd-coredump, apport, WSL's) hands the decision to that handler, which may or may not honour the limit, and piping is the common setting. A process marked not dumpable is never dumped by the kernel, whatever the pattern says, and other processes of the same user can no longer read its memory with `ptrace` or `/proc/<pid>/mem`. If either call fails, the command stops with an error instead of running without it. On other systems only the limit is set. This replaces the instruction in ADR-0022 to set `LimitCORE=0` for the service, which is no longer needed (and does no harm).

The cost of being not dumpable is that the process's `/proc/<pid>` files belong to root, which affects debuggers and tools that read them as the same user. That is the point, and a developer who wants a dump of a crash uses a build or run that does not call it, which does not exist yet: say so when it is needed.

**The user id comes from a safe wrapper.** The workspace forbids `unsafe`, and reading the user id needs a system call. The `hyalos` binary crate takes the `rustix` crate (with its `process` feature only) for it: a thin, widely used wrapper over the system calls that exposes them as safe functions. It is added to `hyalos` only, not to the deterministic crates ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)), and passes `cargo deny`. Reviewing a dependency is part of this decision, not an afterthought, because a crate here is code that runs with the key in memory.

## Consequences

A key another user placed, or kept after changing ownership, is now refused, and a directory shared between users is refused even when its mode is private.

Dumps are off for the whole process, including commands that never hold the key (`--version`, `log verify`), because deciding which commands those are would be one more thing to get wrong.

A test cannot prove the absence of a core file on a machine whose `core_pattern` pipes to a handler, since no file appears there with or without the protection. The test therefore checks what the kernel itself reports, that it dumped no core for a crashed, hardened process, together with the process's limits and dumpable mark, and it also looks for a file where an unhardened control process shows that one would be written.

A test cannot give a file to another user without being root, so the unit tests cover the check by naming a different user for the same files, and a test that really changes the owner runs only where the tests run as root.

A deployment that runs the engine under a different user from the one that made the key has to change the owner once. That is the intended friction.
