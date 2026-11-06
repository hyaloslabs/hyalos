# ADR-0058: The Firecracker microVM substrate

**Status:** Accepted

## Context

[ADR-0011](0011-pluggable-substrates.md) ranks a microVM as the strongest substrate: the job runs on a kernel of its own, and the way out is through the hypervisor, not through a shared kernel's system calls. Story S2.4 (#12) asks for a Firecracker driver that passes the substrate contract ([ADR-0032](0032-the-substrate-interface-and-its-contract.md)), with no network and only the broker reachable.

A container driver gets a lot from its runtime that a VM does not have: an image's filesystem, an init that reaps orphans ([ADR-0041](0041-an-init-in-the-sandbox-and-the-image-tests-further-checks.md)), cgroup limits, a user id, a bind-mounted broker socket, and a way to stream output. Firecracker gives a kernel, block devices, a serial console and vsock. What was found while building it on this host (WSL2, KVM, Firecracker v1.17.0, a 6.1 guest kernel from Firecracker's CI):

- A VM boots and reaches its init in under a second. Restarting or panicking makes Firecracker exit cleanly, which is how a VM ends.
- With `--no-api --config-file`, Firecracker needs nothing but the file: no API socket and no network device.
- vsock alone carries everything. A guest connecting to the host's port P becomes a connection to the Unix socket `<uds>_P` beside the VM, so a symlink there to the broker's socket gives the guest the broker and nothing else.
- An image can become a disk without root: `docker create`, `docker export` into `tar`, then `mke2fs -d` with `root_owner=0:0`. Without `newuidmap` the files' owners cannot be kept, so every file in the guest belongs to root.
- The pinned kernel mounts devtmpfs at boot.
- The jailer, Firecracker's chroot, namespace and privilege-dropping wrapper, needs root.

## Decision

**A driver, `MicroVmSubstrate`, under Podman or Docker.** Its name is `microvm`, its isolation `MicroVm`, and a job on it records `microvm.firecracker` ([ADR-0043](0043-choosing-the-substrate-recording-it-and-doctor.md)). It attests nothing, `attestation.no_hardware_root` ([ADR-0057](0057-recording-a-jobs-attestation.md)), since Firecracker has no confidential mode. The container runtime is used only to read the image.

**The operator names three files in `microvm.conf`**, in the config directory: `firecracker`, `kernel` and `init`, each an absolute path, and nothing else. Without the file, `detect` does not ask the driver and `hyalos doctor` says `microvm: not set up`. With it, `detect_with` asks the driver first under Podman and then Docker, and the strongest ready driver is chosen as before. A file that does not parse stops `run` and `doctor`.

**The image becomes a read-only ext4 disk, made once.** The driver exports the image's filesystem without device nodes, adds the init at `/.hyalos/init` and an `/etc/hosts`, and writes the disk with `mke2fs -d`. The disk is cached under `microvm/` in the data directory, named by the image's id and a hash of the init, and is moved into place only when it is complete. The VM gets it read-only.

**The init is a new crate, `crates/vm-init`.** It is a static binary (`x86_64-unknown-linux-musl`, added to `rust-toolchain.toml`) because it runs on whatever image the job uses. It is not trusted core: it runs inside the sandbox, beside the job, as `hyalos-sandbox-tools` does ([ADR-0017](0017-workspace-layout-and-trusted-core-boundary.md)). It allows `unsafe`, unlike the workspace, since an init must make system calls the standard library does not wrap. Each use has its reason. As PID 1 it:

1. mounts `/proc`, `/sys`, cgroup2, a 64 MiB `/tmp` and a small `/run` (all `nosuid`, `nodev`, `noexec`), keeps the kernel's `/dev`, and remounts `/` read-only, `nosuid` and `nodev`;
2. brings up loopback, the only interface the VM has;
3. connects to the host's vsock ports 1001 and 1002 for stdout and stderr and port 1000 for the job: memory, process limit, a backstop in seconds, working directory, arguments and environment, as NUL-terminated fields under a length and a protocol version;
4. empties the capability bounding set and sets no-new-privileges, so nothing it starts can gain a capability, whatever the image's setuid files;
5. puts the job in a cgroup with `memory.max` (and no swap) and `pids.max`;
6. listens on `/run/broker.sock` and relays each connection to the host's port 1024;
7. runs the program as user and group 65534, with the image's environment and working directory under the spec's environment, and reaps every orphan;
8. when the program ends, kills everything else, reports `exit N`, waits for the host to say it has read the output, and restarts, which ends the VM.

With no arguments it is the readiness probe: it sets up, runs nothing and reports `exit 0`. If the engine is gone, the VM restarts itself 30 seconds after the job's deadline, so a VM does not outlive the engine's interest in it.

**The host side.** Each VM has a directory under `vm/` in the runtime directory (0700) with its configuration, the three listeners and the broker's link. Firecracker runs in its own process group, as the container runtime's client does ([ADR-0053](0053-an-interrupted-run-ends-its-job.md)), with the job's memory plus 128 MiB, 2 vCPUs, the disk read-only and no network device. `wait`, `stop`, the deadline and `StopHandle` kill Firecracker as they kill a container, and the ending is the guest's report, or `Lost` if it gave none. `existing` lists the VMs whose directories remain.

**Ready means a VM boots.** `available` checks the three files, that Firecracker answers, that `/dev/kvm` opens for reading and writing, and that the runtime answers. `ready` adds that the image exists, then boots the probe on a 16 MiB disk holding only the init. That takes about a second, so `detect`'s 60-second timeout is not spent converting the job's image.

**No jailer.** Firecracker runs as the operator, under its own seccomp filters. The jailer would add a chroot, namespaces and a dedicated uid, and it needs root, which Hyalos does not take. The guest is the boundary that counts. If the hypervisor is breached, the job has the operator's uid, the same as the engine; the files it could reach are the ones the jailer would have hidden. This is the tradeoff to revisit when Hyalos runs as a service with its own user.

**Files in the guest belong to root.** Without user-namespace tools the image's owners are lost, so a file the image gives user 65534 is root's in the VM, and the job cannot write it. The root is read-only anyway, and every writable place (`/tmp`, and the work directory the job uses) is made at run time. A setuid file stays setuid but gains nothing, under no-new-privileges and an empty bounding set.

**Tests.**
- `crates/substrate-contract/tests/microvm.rs`: the whole contract (19 checks, no skips) in a microVM on `ubuntu:24.04`, a program that sees a kernel that is not the host's, and a driver with nothing installed that is not available. They skip without `HYALOS_FIRECRACKER`, `HYALOS_VM_KERNEL` and `HYALOS_VM_INIT` unless `HYALOS_REQUIRE_MICROVM` is set.
- The same file: the sandbox image's Codex and Claude Code start in a microVM, and `hyalos-forward`'s port reaches a stand-in broker on the host over vsock (with `HYALOS_REQUIRE_SANDBOX_IMAGE`, in the `sandbox image` workflow).
- `core/hyalos/tests/doctor.rs`: a `microvm.conf` whose files are missing is reported and not chosen, a bad file is refused, and no file gives the advice.
- CI installs the pinned Firecracker (v1.17.0) and kernel (6.1.155), both checked against SHA-256, builds the init, and opens `/dev/kvm` with `.github/scripts/install-firecracker.sh`.

## Consequences

A host with KVM and `microvm.conf` runs every job on its own kernel, and its log says `microvm.firecracker`. That is the boundary ADR-0011 ranks highest, at the cost of a disk per image (about 1.3 times its size) and a slower first job on a new image.

The operator installs three files and needs `/dev/kvm`; there is no daemon to configure. A host without KVM, such as most cloud VMs without nested virtualisation, falls back to gVisor or a container, and `hyalos doctor` says why.

Running without the jailer leaves the operator's uid behind the hypervisor. The full image test (`run_image.rs`) still runs on the container substrates, since it dumps the sandbox's memory with `docker exec`, which cannot reach into a VM. A memory scan of a microVM job is not built.
