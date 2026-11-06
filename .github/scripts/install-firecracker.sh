#!/bin/sh
# Installs the pinned Firecracker release and guest kernel into <dir>, both checked against pinned
# SHA-256 sums, builds the static init, opens /dev/kvm to this user, and writes microvm.conf's three
# paths as HYALOS_FIRECRACKER, HYALOS_VM_KERNEL and HYALOS_VM_INIT to $GITHUB_ENV when it is set
# (ADR-0058). CI runs this; docs/operations/substrates.md has the same steps.
set -eu
DIR="${1:?usage: install-firecracker.sh <dir>}"
V=v1.17.0
FC_SHA256=06094a1108ae9e82aa4c23a775aa92758f53f1175d422270d9d6162cb9ade558
KERNEL=https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.15/x86_64/vmlinux-6.1.155
KERNEL_SHA256=e20e46d0c36c55c0d1014eb20576171b3f3d922260d9f792017aeff53af3d4f2
[ "$(uname -m)" = x86_64 ] || { echo "the pinned checksums are for x86_64" >&2; exit 1; }
mkdir -p "$DIR"
DIR="$(cd "$DIR" && pwd)"
T="$(mktemp -d)"
curl -fsSL -o "$T/firecracker.tgz" "https://github.com/firecracker-microvm/firecracker/releases/download/${V}/firecracker-${V}-x86_64.tgz"
echo "${FC_SHA256}  $T/firecracker.tgz" | sha256sum -c -
tar -xzf "$T/firecracker.tgz" -C "$T"
install -m 755 "$T/release-${V}-x86_64/firecracker-${V}-x86_64" "$DIR/firecracker"
curl -fsSL -o "$T/vmlinux" "$KERNEL"
echo "${KERNEL_SHA256}  $T/vmlinux" | sha256sum -c -
install -m 644 "$T/vmlinux" "$DIR/vmlinux"
rm -rf "$T"
cargo build --release --locked -p hyalos-vm-init --target x86_64-unknown-linux-musl
install -m 755 target/x86_64-unknown-linux-musl/release/hyalos-vm-init "$DIR/hyalos-vm-init"
if ! [ -r /dev/kvm ] || ! [ -w /dev/kvm ]; then
  echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666", OPTIONS+="static_node=kvm"' | sudo tee /etc/udev/rules.d/99-kvm4all.rules >/dev/null
  sudo udevadm control --reload-rules
  sudo udevadm trigger --name-match=kvm
fi
"$DIR/firecracker" --version | head -n 1
if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "HYALOS_FIRECRACKER=$DIR/firecracker"
    echo "HYALOS_VM_KERNEL=$DIR/vmlinux"
    echo "HYALOS_VM_INIT=$DIR/hyalos-vm-init"
  } >> "$GITHUB_ENV"
fi
