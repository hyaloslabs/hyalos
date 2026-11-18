#!/usr/bin/env bash
# Runs one complete job with the release binary before it is published (ADR-0107). $1 is the binary,
# $2 the sandbox image it names by default, which must be on this machine already. No secret is used:
# the model is scripted and GitHub is the tests' stand-in (core/hyalos/tests/release_job.rs). Run
# from the repository root.
set -euo pipefail

binary="$(realpath "$1")"
image="$2"
docker image inspect "$image" >/dev/null

rustup toolchain install
# Line tables only, as CI's tests build, to keep the runner's disk.
CARGO_PROFILE_DEV_DEBUG=line-tables-only \
  HYALOS_RELEASE_JOB_BINARY="$binary" HYALOS_RELEASE_JOB_IMAGE="$image" \
  HYALOS_REQUIRE_GIT=1 HYALOS_REQUIRE_CONTAINER=1 \
  cargo test --locked -p hyalos --test release_job -- --nocapture
