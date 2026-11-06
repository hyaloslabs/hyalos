# ADR-0056: The gVisor substrate

**Status:** Accepted

## Context

[ADR-0011](0011-pluggable-substrates.md) ranks gVisor above a rootless container and below a microVM. [ADR-0032](0032-the-substrate-interface-and-its-contract.md) built only the container driver. A container shares the host's kernel, so a bug in any system call the sandbox can reach is a way out. gVisor puts a kernel of its own, written in Go and running in user space, between the program and the host: the program's system calls are served by gVisor, and gVisor makes far fewer host calls, under its own seccomp filter. That is story S2.2 (#10): a job runs under gVisor with no network, and any outbound connection other than the broker socket fails.

gVisor runs as an OCI runtime, `runsc`, under Docker or Podman, so the container driver's options carry over. Four things found while running the contract under it shape this decision:

- **The broker socket.** By default `runsc` refuses to connect to a Unix socket on the host. The broker's socket is bind-mounted from the host ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)), so it needs `--host-uds=open`, which allows connecting to a host socket that is mounted in, not creating one. A runtime's options are set where it is registered (Docker's `daemon.json`), not per container.
- **The process limit.** `--pids-limit` limits the host cgroup, which under gVisor holds gVisor's own threads, not the program's processes: 150 processes started under a limit of 77, and too low a limit (20) stops gVisor from starting at all. gVisor does enforce `RLIMIT_NPROC` in its own kernel, which counts only the sandbox's processes, since its user ids are its own: 75 forks succeeded and 75 were refused under `nproc=77`, the same as `runc` with the cgroup limit.
- **The memory limit** holds: a program allocating 500 MiB under a 300 MiB limit was killed (exit 137), as under `runc`.
- **What the sandbox can see.** gVisor has no `/sys/class/net` and shows the cgroup limits nowhere a program can read them, so two of the contract's isolation checks read files a driver need not have.

On this WSL2 host gVisor's default platform (`systrap`) failed to start any container (`failed to create a syscall thread`), while `--platform=kvm` and `--platform=ptrace` worked. GitHub's runners use the default.

## Decision

**gVisor is a mode of the container driver.** `ContainerSubstrate::gvisor(runtime)` runs every sandbox with the container driver's options under `--runtime hyalos-runsc`. Its name is `gvisor`, its isolation `UserspaceKernel`, and a job on it records `gvisor.docker` or `gvisor.podman` on `job.started` ([ADR-0043](0043-choosing-the-substrate-recording-it-and-doctor.md)). `detect` asks gVisor under Podman, then under Docker, then the two plain container drivers, and chooses the strongest ready one, so a host with gVisor set up runs every job on it without any configuration in Hyalos.

**The operator registers `runsc` as `hyalos-runsc`**, with `--host-uds=open` and any platform the host needs, as `docs/operations/substrates.md` describes. A name of its own keeps an operator's other `runsc` settings away from jobs. Hyalos never edits the runtime's configuration.

**Ready means the kernel is gVisor's.** The readiness probe runs `uname -r` in place of `true` under the gVisor runtime, with the job's options, and is ready only if the release ends in `-gvisor`. A `hyalos-runsc` that is really `runc`, or that cannot start a container, is not ready, with a message naming what to install and the platform setting. The probe does not check `--host-uds=open`: the runtimes report their options differently, and the contract and the end-to-end job fail without it.

**Processes are limited inside gVisor.** The driver sets `--ulimit nproc=N:N` from the spec's `max_processes`, and keeps a host `--pids-limit` of at least 256 so gVisor can start. `RLIMIT_NPROC` is not used for plain containers: there it would count every process of user 65534 on the host.

**The contract checks limits by what they do.** The limits check now forks well past a limit of 40 processes and expects fewer than 40 and some refused, and allocates 600 MiB under a 300 MiB limit and expects the sandbox to end without saying it has it. The network check reads interfaces from `/proc/net/dev` in place of `/sys/class/net`. The container driver passes both as before.

**Tests.**
- `crates/substrate-contract/tests/gvisor.rs`: the whole contract (19 checks, no skips) under gVisor, a check that the program sees gVisor's kernel and not the host's, and a runtime without gVisor that is not ready. It skips without gVisor unless `HYALOS_REQUIRE_GVISOR` is set.
- `core/hyalos/tests/run_image.rs` records whichever substrate detection chose, and with `HYALOS_REQUIRE_GVISOR` requires `gvisor.<runtime>`. Run on this host, Codex in the sandbox image pushed, opened a pull request and left no credential in the sandbox's environments, memory or `/tmp`, under gVisor.
- CI installs the pinned release (20261019.0, checked against its SHA-512) with `.github/scripts/install-gvisor.sh` and sets `HYALOS_REQUIRE_GVISOR` for the workspace tests and the `sandbox image` workflow.

## Consequences

A Linux host with gVisor set up runs jobs behind a kernel that is not the host's, and its log says so. The boundary is the one gVisor documents, not one Hyalos builds: a gVisor bug, or the host calls gVisor still makes, is the way out, and the upgrade path is the pinned release.

Setting up gVisor is the operator's, with `sudo`: installing `runsc` and registering it with the runtime. A host that gets it wrong, such as `systrap` on WSL2, falls back to the container driver and `hyalos doctor` says why. A `hyalos-runsc` without `--host-uds=open` passes the probe but every job fails to reach the broker, which the operator sees as a job that cannot call the model.

The process limit under gVisor is a different mechanism from a container's, chosen because it is the one that works there. Only Docker has been tested; the Podman path is the same options and is untested.

Not here: gVisor's own network stack (jobs keep `--network none`), and the microVM driver (S2.4, #12).
