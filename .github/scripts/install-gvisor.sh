#!/bin/sh
# Installs the pinned gVisor release and registers it with Docker as hyalos-runsc, the runtime the
# gVisor substrate runs every sandbox under (ADR-0056). The release is checked against its pinned
# SHA-512 before anything is installed. CI runs this; docs/operations/substrates.md has the same steps.
set -eu
V=20261019.0
SHA512=7cbc2aeeee809dcccfb40e612504a1b76f2c119790cee59e482f7b35428cccc4f71705e7e322442d07d1f7d29b5d9e72e31319fc3088d6dcbec9fc7c1f0d8881
[ "$(uname -m)" = x86_64 ] || { echo "the pinned checksum is for x86_64" >&2; exit 1; }
D="$(mktemp -d)"
cd "$D"
curl -fsSLO "https://storage.googleapis.com/gvisor/releases/release/${V}/x86_64/gvisor.tar.bz2"
echo "${SHA512}  gvisor.tar.bz2" | sha512sum -c -
tar xjf gvisor.tar.bz2
sudo install -m 755 runsc containerd-shim-runsc-v1 /usr/local/bin
sudo install -d -m 755 /usr/local/bin/gvisor-bin
sudo install -m 755 gvisor-bin/* /usr/local/bin/gvisor-bin/
sudo /usr/local/bin/runsc install --runtime=hyalos-runsc --download-sidecars=NEVER -- --host-uds=open "$@"
sudo systemctl reload docker
/usr/local/bin/runsc --version | head -n 1
