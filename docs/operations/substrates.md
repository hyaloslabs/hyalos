# Substrates

A job runs on the strongest substrate this host has ready ([ADR-0011](../adr/0011-pluggable-substrates.md)), and `job.started` records which. `hyalos doctor` lists each one and why it is or is not ready.

| Substrate | Recorded as | Isolation | Needs |
|---|---|---|---|
| gVisor | `gvisor.docker`, `gvisor.podman` | gVisor's kernel in user space between the job and the host's | `runsc` registered as `hyalos-runsc` |
| Container | `container.docker`, `container.podman` | the host's kernel, behind namespaces and dropped privileges | Docker or Podman with its init |

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
