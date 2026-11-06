# Substrates

A job runs on the strongest substrate this host has ready ([ADR-0011](../adr/0011-pluggable-substrates.md)), and `job.started` records which. `hyalos doctor` lists each one and why it is or is not ready.

| Substrate | Recorded as | Isolation | Needs |
|---|---|---|---|
| MicroVM | `microvm.firecracker` | its own guest kernel in a Firecracker virtual machine on KVM, with no network device | `/dev/kvm`, Firecracker, a guest kernel and `hyalos-vm-init`, named in `microvm.conf`; Docker or Podman holds the image |
| gVisor | `gvisor.docker`, `gvisor.podman` | gVisor's kernel in user space between the job and the host's | `runsc` registered as `hyalos-runsc` |
| Container | `container.docker`, `container.podman` | the host's kernel, behind namespaces and dropped privileges | Docker or Podman with its init |

## MicroVM

Each job runs in its own Firecracker virtual machine, on a guest kernel, with KVM between it and the host ([ADR-0058](../adr/0058-the-firecracker-microvm-substrate.md)). This is the strongest boundary Hyalos has, and a job runs in one whenever one is set up and ready. The VM has no network device: the guest reaches the host only through vsock, and only the broker's socket is wired to it.

**Install** (x86_64 Linux with KVM; `sudo` only to open `/dev/kvm`). These are the release and kernel CI tests, checked against their SHA-256; `.github/scripts/install-firecracker.sh <dir>` does the same, from a checkout of this repository.

```
DIR=/opt/hyalos-microvm   # anywhere you own
V=v1.17.0
curl -fsSLO "https://github.com/firecracker-microvm/firecracker/releases/download/${V}/firecracker-${V}-x86_64.tgz"
echo "06094a1108ae9e82aa4c23a775aa92758f53f1175d422270d9d6162cb9ade558  firecracker-${V}-x86_64.tgz" | sha256sum -c -
tar xzf "firecracker-${V}-x86_64.tgz"
install -m 755 "release-${V}-x86_64/firecracker-${V}-x86_64" "$DIR/firecracker"
curl -fsSL -o vmlinux https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.15/x86_64/vmlinux-6.1.155
echo "e20e46d0c36c55c0d1014eb20576171b3f3d922260d9f792017aeff53af3d4f2  vmlinux" | sha256sum -c -
install -m 644 vmlinux "$DIR/vmlinux"
cargo build --release --locked -p hyalos-vm-init --target x86_64-unknown-linux-musl
install -m 755 target/x86_64-unknown-linux-musl/release/hyalos-vm-init "$DIR/hyalos-vm-init"
```

Then say where they are in `microvm.conf` in the config directory (beside `engine.pub`). All three are required and must be absolute paths:

```
firecracker = /opt/hyalos-microvm/firecracker
kernel = /opt/hyalos-microvm/vmlinux
init = /opt/hyalos-microvm/hyalos-vm-init
```

- **KVM.** You must be able to open `/dev/kvm` for reading and writing. Add yourself to the `kvm` group (`sudo usermod -aG kvm $USER`, then log in again). On WSL2, `/dev/kvm` exists when nested virtualisation is on; `sudo chmod 666 /dev/kvm` lasts until the next restart.
- **The image** still comes from Docker or Podman. The first job on an image turns it into a read-only disk (about 1.3 times its size) under `microvm/` in the data directory, kept by the image's id and the init. Delete that directory to reclaim the space; the next job makes the disk again. `mke2fs` (e2fsprogs) must be installed.
- **The kernel** must have vsock, virtio block, ext4, devtmpfs and cgroup v2 with the memory and pids controllers. The pinned one does.
- **Each VM** has the job's memory plus 128 MiB for its kernel and init, and 2 vCPUs. Its sockets are under `vm/` in the runtime directory while it runs.

**Check it.** `hyalos doctor` should say `substrate microvm.firecracker: ready (micro-VM isolation)` and `chosen: microvm.firecracker`. Without `microvm.conf` it says `microvm: not set up`. The readiness check boots a tiny VM, which takes about a second.

**What runs where.** Firecracker runs as you, with its own seccomp filters, but not under its jailer, which needs root ([ADR-0058](../adr/0058-the-firecracker-microvm-substrate.md)). Inside, `hyalos-vm-init` is PID 1: it mounts a read-only root, private `/tmp` and `/run`, sets the job's memory and process limits in a cgroup, drops every capability from the bounding set, sets no-new-privileges, and runs the program as user 65534. If the engine is gone, the VM ends itself 30 seconds after the job's deadline.

**Upgrading.** Pin a new release or kernel by its SHA-256, here and in `.github/scripts/install-firecracker.sh`, and run the contract (`cargo test -p hyalos-substrate-contract --test microvm` with `HYALOS_REQUIRE_MICROVM=1` and `HYALOS_FIRECRACKER`, `HYALOS_VM_KERNEL` and `HYALOS_VM_INIT` set).

## gVisor

gVisor serves the job's system calls from a kernel of its own, so a bug in the host kernel is much harder for a job to reach ([ADR-0056](../adr/0056-the-gvisor-substrate.md)). Hyalos runs it under the runtime name `hyalos-runsc`, which you register once. Hyalos never changes the runtime's configuration itself.

**Install with Docker** (x86_64; needs `sudo`). This is the release CI tests, checked against its SHA-512; `.github/scripts/install-gvisor.sh` does the same.

```
V=20261019.0
curl -fsSLO "https://storage.googleapis.com/gvisor/releases/release/${V}/x86_64/gvisor.tar.bz2"
echo "7cbc2aeeee809dcccfb40e612504a1b76f2c119790cee59e482f7b35428cccc4f71705e7e322442d07d1f7d29b5d9e72e31319fc3088d6dcbec9fc7c1f0d8881  gvisor.tar.bz2" | sha512sum -c -
tar xjf gvisor.tar.bz2
sudo install -m 755 runsc containerd-shim-runsc-v1 /usr/local/bin
sudo install -d -m 755 /usr/local/bin/gvisor-bin
sudo install -m 755 gvisor-bin/* /usr/local/bin/gvisor-bin/
sudo /usr/local/bin/runsc install --runtime=hyalos-runsc --download-sidecars=NEVER -- --host-uds=open
sudo systemctl reload docker
```

- `runsc` needs the programs in `gvisor-bin/` beside it.
- `--host-uds=open` lets the job connect to the broker's socket, which is mounted in from the host. Without it the job starts but cannot reach the broker, so it cannot call the model or the forge.
- The entry goes in `/etc/docker/daemon.json`. A reload picks it up without stopping running containers.

**WSL2, and other kernels where gVisor starts nothing.** If `hyalos doctor` says gVisor cannot start a container (`urpc method "containerManager.StartRoot" failed: EOF`), gVisor's default way of catching system calls does not work on this kernel. Choose another one: `--platform=kvm` if `/dev/kvm` exists, else `--platform=ptrace` (slower):

```
sudo /usr/local/bin/runsc install --runtime=hyalos-runsc --download-sidecars=NEVER -- --host-uds=open --platform=kvm
sudo systemctl reload docker
```

**Podman.** The driver runs `podman run --runtime hyalos-runsc` with the same options. Podman has no per-runtime arguments, so register a small wrapper as `hyalos-runsc` in `containers.conf` (`[engine.runtimes]`) that runs `runsc --host-uds=open "$@"`. This path is untested.

**Check it.** `hyalos doctor` should say `substrate gvisor.docker: ready (userspace kernel isolation)` and `chosen: gvisor.docker`. To find out why gVisor is not ready, run `docker run --rm --runtime hyalos-runsc ubuntu:24.04 dmesg`; it should print `Starting gVisor...`.

**Limits.** Under gVisor the job's processes live in gVisor's kernel, not the host's, so the driver limits them there (`RLIMIT_NPROC`, the job's process limit). The host's process limit then only bounds gVisor's own threads. Memory is limited on the host as for a container.

**Upgrading.** Pin a new release by version and SHA-512 (the `.sha512` file beside the tarball), here and in `.github/scripts/install-gvisor.sh`, and run the contract (`cargo test -p hyalos-substrate-contract --test gvisor` with `HYALOS_REQUIRE_GVISOR=1`).

## Containers

The container driver needs Docker or Podman, the sandbox image, and the runtime's init ([ADR-0041](../adr/0041-an-init-in-the-sandbox-and-the-image-tests-further-checks.md)). A rootless Podman is a stronger boundary than a Docker daemon running as root. `hyalos doctor` names what to install when the init is missing.
