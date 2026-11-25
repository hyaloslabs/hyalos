#!/usr/bin/env bash
# Builds the spike's MicroPython guest, spikes/438/guests/micropython.wasm (Slice 3, 3.1, of
# docs/plans/hyalos-kernel-python).
#
#   spikes/438/micropython/build.sh
#
# 1. Fetches MicroPython's release tag from its repository and stops unless the tag is the pinned
#    commit. A tree fetched before is reused when its commit is the pinned one, so a later run
#    needs no network once the toolchain image exists.
# 2. Builds the toolchain image (Containerfile): the host needs only docker (or CONTAINER=podman).
# 3. Builds the port twice, each time from a clean copy of the pinned tree at the same path, with
#    SOURCE_DATE_EPOCH=0, and stops unless the two guests are byte-identical.
# 4. Writes the guest and prints its size and SHA-256, which README.md records.
#
# The build is `make` in ports/hyalos (port/Makefile), then Binaryen's Asyncify pass, which gives
# MicroPython's setjmp and longjmp a way to unwind and rewind the stack in plain WebAssembly
# (port/sjlj.c). `asyncify-imports` names no real import, so no host call may unwind, and only
# `run`, the loop that resumes an unwind, is left uninstrumented. Asyncify runs before `-Os`, since
# optimising first would inline the functions it must find by name.
set -euo pipefail

URL=https://github.com/micropython/micropython
TAG=v1.29.0
SHA=0fd6c573ea815774668bbb16b8e197c8822368b2

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
container=${CONTAINER:-docker}
image=hyalos-spike-438-micropython:1
cache=${HYALOS_SPIKE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/hyalos-spike-438}
src="$cache/micropython-src"
work="$cache/micropython-build"
out="$root/spikes/438/guests/micropython.wasm"

fail() { echo "build.sh: $*" >&2; exit 1; }

mkdir -p "$cache"
if [ "$(git -C "$src" rev-parse -q --verify HEAD 2>/dev/null || true)" != "$SHA" ]; then
  rm -rf "$src"
  git init -q "$src"
  git -C "$src" fetch -q --depth 1 "$URL" "refs/tags/$TAG:refs/tags/$TAG" \
    || fail "cannot fetch $TAG from $URL"
  got=$(git -C "$src" rev-parse "$TAG^{commit}")
  [ "$got" = "$SHA" ] || fail "$TAG is $got at $URL, not the pinned $SHA"
  git -C "$src" checkout -q --detach "$SHA"
fi
[ "$(git -C "$src" rev-parse HEAD)" = "$SHA" ] || fail "the fetched tree is not at $SHA"

"$container" build -q -t "$image" -f "$here/Containerfile" "$here" >/dev/null

# One build from a clean tree at $work; the guest goes to $1.
build() {
  rm -rf "$work"
  mkdir -p "$work"
  git -C "$src" archive "$SHA" | tar -x -C "$work"
  cp -R "$here/port" "$work/ports/hyalos"
  "$container" run --rm -u "$(id -u):$(id -g)" -e SOURCE_DATE_EPOCH=0 \
    -v "$work:/micropython" -w /micropython/ports/hyalos "$image" sh -ec '
      make -j"$(nproc)" >&2
      wasm-opt build/micropython.raw.wasm --asyncify \
        --pass-arg=asyncify-imports@hyalos.none --pass-arg=asyncify-removelist@run \
        -Os -o build/micropython.wasm'
  cp "$work/ports/hyalos/build/micropython.wasm" "$1"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
build "$tmp/first.wasm"
build "$tmp/second.wasm"
cmp -s "$tmp/first.wasm" "$tmp/second.wasm" || fail "the two builds differ"

mkdir -p "$(dirname "$out")"
cp "$tmp/first.wasm" "$out"
echo "micropython.wasm: $(wc -c <"$out") bytes, sha256 $(sha256sum "$out" | cut -d ' ' -f 1)"
