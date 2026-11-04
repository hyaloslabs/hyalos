# ADR-0041: An init in the sandbox, and the image test's further checks

**Status:** Accepted

## Context

The container substrate ([ADR-0032](0032-the-substrate-interface-and-its-contract.md)) runs the job's program as the container's entrypoint, which makes it PID 1. In the sandbox image ([ADR-0040](0040-the-sandbox-image.md)) that program is `hyalos-forward`. A process whose parent exits is handed to PID 1, which must collect it when it ends. `hyalos-forward` uses only the standard library and collects only the harness, so a command a harness's Bash tool puts in the background, once orphaned, stays a zombie after it ends, and every zombie counts against the sandbox's process limit. PID 1 also ignores any signal it has no handler for, so a `SIGTERM` sent to the sandbox did nothing.

The review of ADR-0040 also found:
- two gaps in its test: the DNS log listened over UDP only, and no control showed that Claude Code's settings take effect;
- that nothing yet exercises `skipWebFetchPreflight`.

These are story S2.9b (#166).

## Decision

**The container substrate passes `--init`.** The runtime puts its own minimal init in as PID 1 and runs the program under it:
- Docker uses `docker-init`, the tini build that ships with the engine.
- Podman uses the init named by `init_path` in its `containers.conf`, `catatonit` by default, which must be installed. Where it is missing, the runtime refuses to start the sandbox: a job cannot run, but nothing runs without the init.

The init collects every orphan and passes signals on to the program. The image does not ship one of its own, because the substrate sets the entrypoint and an init chosen by the substrate covers every image.

The contract ([`crates/substrate-contract`](../../crates/substrate-contract)) gains an isolation check, "an init is PID 1 and reaps orphaned processes":
- the program must not be PID 1;
- once a process has been orphaned and has ended, no zombie may remain.

Without `--init`, the check fails. The image test runs its containers with `--init` too, matching the substrate.

**The DNS log also answers over TCP.** `hyalos-dns-log` listens on `127.0.0.1:53` for TCP as well as UDP (`DnsLog::serve_tcp`), reading each two-byte length and query and recording the name as for UDP. A resolver that falls back to TCP is therefore seen.

**The image test has a control for Claude Code.** With `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` cleared and the other three Claude settings still set, Claude Code sends `CONNECT api.anthropic.com:443` on the channel, and the test requires that call to appear. This shows the setting is the one that stops it, and that the channel check would see it.

**`skipWebFetchPreflight` waits for the egress proxy.** The test cannot show it works until a step can use web fetch, which needs the broker's allowlisting proxy (S3.5, #20). That check moves to #20.

## Consequences

Orphaned processes no longer pile up against a job's process limit, and a `SIGTERM` reaches the program. A host running Podman needs `catatonit` (or the init its configuration names). Nothing checks for it before a job starts: `ContainerSubstrate::available` does not, so a Podman without it refuses every job. Whatever first starts the engine should check for it. Each of the image test's checks now has a control showing it can see a call: the channel and the DNS log for Codex, and the channel for Claude Code. The DNS log still cannot see a connection to an IP literal; `--network none` refuses those in a job.
