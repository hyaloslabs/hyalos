#!/usr/bin/env bash
# Builds the spike's Monty guest, spikes/438/guests/monty.wasm (Slice 4, 4.1, of
# docs/plans/hyalos-kernel-python).
#
#   spikes/438/monty/build.sh
#
# 1. Fetches Monty's release tag v1.6.0 (fetch.sh), the tag the plan names; it exists.
# 2. Builds the guest crate (guest/) twice, each time from a clean copy of the pinned tree at the
#    same path, with SOURCE_DATE_EPOCH=0 and `--locked`, and stops unless the two guests are
#    byte-identical.
# 3. Writes the guest and prints its size, its SHA-256 and the rustc that built it, which
#    README.md records.
#
# The guest is a crate of its own, copied into the tree as `hyalos-guest/`, whose path
# dependencies are Monty's `monty` and `monty-types`. Its Cargo.lock is Monty's own, pruned by
# Cargo to the packages this target and these features reach: every registry package in it is at
# the version Monty's Cargo.lock pins (checked by `lock_matches`, fetch.sh). The build configuration that
# differs from Monty's, and nothing in Monty's sources:
#
# - `chrono` without its default features (`clock` and `wasmbind`, which read JavaScript's clock
#   on this target), keeping `std`, `now` and Monty's `serde`: one line of Monty's workspace
#   Cargo.toml, edited here in the copy;
# - `web-time` replaced by guest/web-time, a clock that never moves, through `[patch.crates-io]`;
# - `getrandom`'s custom backend (`--cfg getrandom_backend="custom"`), which the guest supplies as a
#   constant.
#
# So the guest imports the four `hyalos` functions and nothing else: no WASI, no JavaScript.
set -euo pipefail

TARGET=wasm32-unknown-unknown

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
out="$root/spikes/438/guests/monty.wasm"
cargo_home=${CARGO_HOME:-$HOME/.cargo}

fail() { echo "build.sh: $*" >&2; exit 1; }
# shellcheck source=fetch.sh
. "$here/fetch.sh"
work="$cache/monty-build"

rustup run "$TOOLCHAIN" rustc --print target-list | grep -qx "$TARGET" \
  || fail "rustc $TOOLCHAIN does not know $TARGET"
rustup target list --toolchain "$TOOLCHAIN" --installed | grep -qx "$TARGET" \
  || fail "install the target: rustup target add --toolchain $TOOLCHAIN $TARGET"

lock_matches "$here/guest/Cargo.lock" || fail "the guest's lockfile is not Monty's"

# One build from a clean tree at $work; the guest goes to $1.
build() {
  rm -rf "$work"
  mkdir -p "$work"
  git -C "$src" archive "$SHA" | tar -x -C "$work"
  cp -R "$here/guest" "$work/hyalos-guest"
  rm -rf "$work/hyalos-guest/target"
  local old='chrono = { version = "0.4", features = ["serde"] }'
  local new='chrono = { version = "0.4", default-features = false, features = ["serde", "std", "now"] }'
  grep -qxF "$old" "$work/Cargo.toml" || fail "Monty's Cargo.toml has no line: $old"
  python3 -I -c 'import sys; p, o, n = sys.argv[1:]; t = open(p).read(); open(p, "w").write(t.replace(o + "\n", n + "\n", 1))' \
    "$work/Cargo.toml" "$old" "$new"
  (
    cd "$work/hyalos-guest"
    SOURCE_DATE_EPOCH=0 RUSTUP_TOOLCHAIN="$TOOLCHAIN" \
      RUSTFLAGS="--cfg getrandom_backend=\"custom\" --remap-path-prefix=$work=/monty --remap-path-prefix=$cargo_home=/cargo" \
      cargo build --release --locked --target "$TARGET" >&2
  )
  cp "$work/hyalos-guest/target/$TARGET/release/hyalos_monty_guest.wasm" "$1"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
build "$tmp/first.wasm"
build "$tmp/second.wasm"
cmp -s "$tmp/first.wasm" "$tmp/second.wasm" || fail "the two builds differ"

mkdir -p "$(dirname "$out")"
cp "$tmp/first.wasm" "$out"
echo "monty.wasm: $(wc -c <"$out") bytes, sha256 $(sha256sum "$out" | cut -d ' ' -f 1)"
echo "built by: $(rustup run "$TOOLCHAIN" rustc --version), target $TARGET"
