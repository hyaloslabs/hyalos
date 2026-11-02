#!/bin/bash
# Builds sigstore-rust's own verify_bundle example at a pinned tag (needs a network and Docker).
# The binary lands in run/rust/. Run this once; run-rust.sh then needs no network.
cd "$(dirname "$0")"
TAG="${SIGSTORE_RUST_TAG:-sigstore-verify-v0.15.1}"
mkdir -p run/rust
docker run --rm -v "$PWD/run/rust":/out -e TAG="$TAG" rust:1 bash -c '
  set -e
  git clone -q --depth 1 --branch "$TAG" https://github.com/sigstore/sigstore-rust /src
  cd /src && git rev-parse HEAD > /out/commit.txt
  cargo build -q --release -p sigstore-verify --example verify_bundle
  cp target/release/examples/verify_bundle /out/verify_bundle' && echo "built $TAG at $(cat run/rust/commit.txt)"
