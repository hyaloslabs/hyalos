#!/usr/bin/env bash
# Builds the hyalOS kernel image a profile's KVM runs need and prints its path (Slice 2, 2.5, of
# docs/plans/hyalos-kernel-python).
#
#   spikes/438/kernel.sh <profile>
#
# `hyalos.profile/2` and `hyalos.spike/1` run on the default image, which is built as the monitor's
# own tests build it (`hyalos-os/monitor/tests/release_switches.rs`, `built_kernel`). A spike
# profile with other settings gets its image from the kernel image feature the slice that adds it
# names here. Any other profile exits non-zero. Cargo's output goes to standard error, so standard
# output is the path alone.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
profile=${1:-}
case "$profile" in
  hyalos.profile/2 | hyalos.spike/1) features=() target=target ;;
  # Slice 4: Monty's profile, with floats (ADR-0137). Its own target directory, so it never
  # replaces the default image.
  hyalos.spike/2) features=(--features spike-profile-2) target=target-spike-2 ;;
  "") echo "usage: kernel.sh <profile>" >&2; exit 2 ;;
  *) echo "kernel.sh: no kernel image is built for profile '$profile'" >&2; exit 1 ;;
esac

dir="$root/hyalos-os/kernel-image"
# A cargo run that calls this script must not hand its own settings to the kernel image's build.
for name in $(env | sed -n 's/^\(CARGO[A-Z_]*\|RUSTFLAGS\|RUSTC\)=.*/\1/p'); do
  [ "$name" = CARGO_HOME ] || unset "$name"
done
(cd "$dir" && cargo build --release --locked --target-dir "$target" "${features[@]}") >&2
image="$dir/$target/x86_64-unknown-none/release/hyalos-kernel"
[ -s "$image" ] || { echo "kernel.sh: the build wrote no image" >&2; exit 1; }
echo "$image"
