# ADR-0032: The substrate interface and its contract

**Status:** Proposed

## Context

[ADR-0011](0011-pluggable-substrates.md) decides that sandboxes run on a pluggable substrate (microVM, then gVisor, then rootless container), that the sandbox has no network and only a socket to the broker, that the engine picks the best the host has and records which one ran each job, and that every driver passes the same contract suite. [ADR-0002](0002-one-binary-two-processes-sandboxes.md) puts secrets outside the sandbox. They leave the interface itself and the contract open. This ADR adds both for story S2.1 and the first real driver. It changes nothing those two decided.

## Decision

**The interface** (`hyalos-substrate`, which depends only on `hyalos-types`). A `Substrate` has a `name`, an `isolation` level (container < userspace kernel < microVM, so the engine can compare), `available` (can this driver run here now, without starting anything), `start(&SandboxSpec)` and `existing` (the sandboxes it started that still exist). A `Sandbox` has an `id`, `wait` (returns when the sandbox is gone) and `stop`. A sandbox that starts and then fails is not an error: it ends with an `Outcome` whose `Ending` is `Exited(code)`, `DeadlineExceeded`, `Stopped` or `Lost`. A sandbox that is dropped without being waited for is killed and removed.

**The spec** is what the engine asks for: the job, an image, a command, an environment, the broker's socket on the host, a deadline, a memory limit and a process limit. There is no way to ask for a network, a host mount, a device or a privilege, because the interface has no field for them. The environment is the whole environment and nothing is inherited from the host. It is visible to anyone who can inspect the sandbox, so it holds settings and placeholders, never a secret ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)). The broker's socket appears at `/run/broker.sock` in every sandbox, whatever the driver, so the forwarder ([ADR-0016](0016-agent-runtime-and-broker-protocols.md)) has one path to connect to.

**Validation is in the spec, not in each driver.** `SandboxSpec::validate` refuses an image that could be read as a runtime option (empty, leading `-`, characters an image reference does not have), a command that is empty or has a NUL, an environment name that is not a name, a socket path that is not absolute or holds a character a mount option reads as syntax, and a zero deadline or limit. Every driver calls it first, and the contract checks each case, so a bad spec does the same thing everywhere. Drivers run the command directly, never through a shell.

**Output is bounded.** A sandbox's stdout and stderr are each kept to 64 KiB. The rest is read and thrown away, so a sandbox cannot make the engine hold unbounded memory or block on a full pipe.

**The container driver** runs the `docker` or `podman` command (podman first: no root daemon). Each sandbox gets no network at all (`--network none`), a read-only root with a small private non-executable `/tmp`, user `nobody` (65534), all capabilities dropped, no new privileges, memory (with swap equal) and process limits, no image pulling during a job, no runtime log of its output, an entrypoint set to the command's program so the image's own cannot stand in front of it, and a label naming the job. It is the weakest substrate ([ADR-0011](0011-pluggable-substrates.md)) and says so (`Isolation::Container`). It is chosen only as the lowest rung; gVisor behind the same runtime is S2.2.

**The contract** (`crates/substrate-contract`, not trusted core) is a function that takes any driver and returns a report of what passed, failed and was skipped. It has two halves. *Behaviour*, for every driver: exit codes, captured and bounded output, the environment being the spec's and nothing from the host, no shell, the deadline kills, stopping and its idempotence, nothing left after a sandbox ends or is dropped, bad specs refused with nothing started, a missing image refused. *Isolation*, for any driver that claims to isolate, checked from inside the sandbox: only a loopback interface and no route out, not root, a read-only root and a writable `/tmp`, no capabilities and no new privileges, the broker's socket reachable and answering, and the memory and process limits applied. A driver that does not claim to isolate skips the second half and the report lists each skip: a skip is never a pass.

**The fake** is `LocalProcessSubstrate`, which runs the program as a host process with an empty environment. It isolates nothing and exists so the behavioural half runs anywhere and so the contract itself is tested against a second implementation. The contract found a real defect in it while it was written (killing the shell left a grandchild holding the output open), which is the point of having one.

**CI.** The container half runs on the GitHub runner, which has Docker. CI pulls `ubuntu:24.04` and sets `HYALOS_REQUIRE_CONTAINER=1`, so there a missing runtime fails the test; on a machine without a runtime the test skips with a message, and does not pass silently while CI is green.

## Consequences

The engine can ask any driver for the same thing and compare their strength, and a new driver (gVisor, microVM, macOS) is done when it passes the same suite. The isolation half is only as strong as its checks: it shows the boundary the driver set up from inside, not that the kernel it shares is safe, which is why this driver ranks last.

Because the container driver shells out to a runtime, a host whose runtime is a root Docker daemon gives a weaker boundary than a rootless Podman; the driver reports `Container` either way and the operator picks the runtime. Rootless-only enforcement is a later decision.

`start` is blocking and the sandbox's wait polls at 10 ms; the engine's use of it (a worker per job) is a later story. The sandbox cannot yet be given files other than the broker socket: how a workspace reaches it is S2.3.
