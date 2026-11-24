#!/usr/bin/env bash
# Builds the static `hyalos-verify` and its SHA256SUMS into $1 (default dist), from the root of a git
# clone (ADR-0131). The image is named by digest and the C compiler by version, and the clone is
# mounted at one path, so the same commit gives the same bytes in any directory.
set -euo pipefail

out="${1:-dist}"
builder="rust:1.99.1-slim-bookworm@sha256:ed2c95ca377d6571b124b05e75e01207973ded639dce7d5542eb7bcd797e32aa"
musl="1.2.3-1"
target="x86_64-unknown-linux-musl"
binary="hyalos-verify-x86_64-linux"

# Only when this directory is the top of a clone: inside another repository, HEAD is not this source's
# commit. Otherwise the binary says its commit is unknown, and a release refuses it.
commit=""
if [ "$(git rev-parse --show-toplevel 2>/dev/null)" = "$(pwd -P)" ]; then
  commit="$(git rev-parse HEAD)"
fi
mkdir -p "$out"
out="$(cd "$out" && pwd)"

docker run --rm -v "$PWD:/src:ro" -v "$out:/out" -w /src/hyalos-os \
  -e HYALOS_BUILD_COMMIT="$commit" -e TARGET="$target" -e MUSL="$musl" -e BINARY="$binary" \
  -e CARGO_TARGET_DIR=/tmp/target \
  -e CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="--remap-path-prefix=/src=/hyalos --remap-path-prefix=/usr/local/cargo=/cargo" \
  -e CFLAGS_x86_64_unknown_linux_musl="-ffile-prefix-map=/src=/hyalos -ffile-prefix-map=/usr/local/cargo=/cargo" \
  "$builder" bash -c '
  set -euo pipefail
  apt-get update -qq && apt-get install -y -qq --no-install-recommends "musl-tools=$MUSL" >/dev/null
  rustup target add "$TARGET"
  cargo build --release --locked -p hyalos-verify --target "$TARGET"
  install -m 755 "$CARGO_TARGET_DIR/$TARGET/release/hyalos-verify" "/out/$BINARY"
  chown "$(stat -c %u:%g /out)" "/out/$BINARY"'

file "$out/$binary" | grep -q 'static' || { file "$out/$binary"; exit 1; }
"$out/$binary" --version
(cd "$out" && sha256sum "$binary" > SHA256SUMS && cat SHA256SUMS)
