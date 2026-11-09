#!/usr/bin/env bash
# Builds the static `hyalos` for a release into dist/ and checks it (ADR-0081). $1 is the sandbox
# image by digest the binary names as its default, or empty for a check build, which then names
# hyalos-sandbox:latest. Run from the repository root.
set -euo pipefail

image="${1:-}"
builder="rust:1.99.1-slim-bookworm@sha256:ed2c95ca377d6571b124b05e75e01207973ded639dce7d5542eb7bcd797e32aa"
target="x86_64-unknown-linux-musl"

# Static, with musl, so it runs on any x86_64 Linux. The C parts (ring, SQLite, aws-lc) need musl-gcc.
# In the Containerfile's own builder, so the binary and the sandbox tools build alike.
docker run --rm -v "$PWD:/src" -w /src -e HYALOS_RELEASE_IMAGE="$image" -e TARGET="$target" "$builder" bash -c '
  set -euo pipefail
  apt-get update -qq && apt-get install -y -qq --no-install-recommends musl-tools >/dev/null
  rustup target add "$TARGET"
  cargo build --release --locked -p hyalos --target "$TARGET"
  # The policy engine answers on the thread the binary runs its commands on (#280). Only a musl
  # build shows it, and only off a test harness thread.
  cargo test --release --locked -p hyalos --target "$TARGET" --test main_thread
  chown -R "$(stat -c %u:%g /src)" target'
mkdir -p dist
cp "target/$target/release/hyalos" dist/hyalos-x86_64-linux

# It is static, it runs, and it names the image it was built with.
file dist/hyalos-x86_64-linux | grep -q 'static' || { file dist/hyalos-x86_64-linux; exit 1; }
dist/hyalos-x86_64-linux --version
if [ -n "$image" ]; then
  want="docker pull $image"
else
  want="docker build -f images/sandbox/Containerfile"
fi
scratch="$(mktemp -d)"
# setup exits 1 while steps are not done; what it prints is checked, not its status.
HYALOS_CONFIG_DIR="$scratch/c" HYALOS_DATA_DIR="$scratch/d" \
  dist/hyalos-x86_64-linux setup --json > "$scratch/setup.json" || true
grep -qF "$want" "$scratch/setup.json" || { cat "$scratch/setup.json"; exit 1; }

(cd dist && sha256sum hyalos-x86_64-linux > SHA256SUMS && cat SHA256SUMS)
