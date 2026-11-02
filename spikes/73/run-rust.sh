#!/bin/bash
# The same matrix as run-python.sh, through sigstore-rust's verify_bundle, in a container with no network.
#   ./rust-build.sh && python3 variants.py history && ./run-rust.sh
cd "$(dirname "$0")"
[ -x run/rust/verify_bundle ] || { echo "run ./rust-build.sh first" >&2; exit 1; }
[ -d run/hist ] || python3 variants.py history || exit 1
ID=keyless@projectsigstore.iam.gserviceaccount.com
ISS=https://accounts.google.com
ROOT=vectors/trusted_root.2026-11-02.json

verify() {  # label  trusted-root  bundle  artifact  identity
  local out rc r
  out=$(docker run --rm --network none -v "$PWD":/w:ro -w /w ubuntu:24.04 run/rust/verify_bundle \
    --trusted-root "$2" --certificate-identity "$5" --certificate-oidc-issuer "$ISS" "$4" "$3" 2>&1); rc=$?
  if [ $rc -eq 0 ]; then r="accepted"; else r="refused: $(echo "$out" | grep -v '^\s*$' | tail -1 | cut -c1-70)"; fi
  printf '  %-42s %s\n' "$1" "$r"
}

V=vectors
echo "sigstore-rust $(cat run/rust/commit.txt | cut -c1-10)"
echo "== main matrix (current snapshot)"
verify "fresh bundle (2026-10-20)"      $ROOT $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json $V/cosign-v3.1.4/cosign_checksums.txt $ID
verify "year-old bundle (2025-10-08)"   $ROOT $V/cosign-v3.0.1/cosign_checksums.txt.sigstore.json $V/cosign-v3.0.1/cosign_checksums.txt $ID
verify "tampered artifact"              $ROOT $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json run/tamper/cosign_checksums.txt $ID
verify "wrong identity"                 $ROOT $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json $V/cosign-v3.1.4/cosign_checksums.txt attacker@example.com
echo "== evidence removed from the fresh bundle"
for f in run/strip/*.sigstore.json; do
  verify "$(basename "$f" .sigstore.json)" $ROOT "$f" $V/cosign-v3.1.4/cosign_checksums.txt $ID
done
echo "== old trusted roots (rows) against each bundle"
for r in $(ls run/hist/*.root.json | sort -V); do
  n=$(basename "$r" .root.json)
  verify "$n / fresh"     "$r" $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json $V/cosign-v3.1.4/cosign_checksums.txt $ID
  verify "$n / year-old"  "$r" $V/cosign-v3.0.1/cosign_checksums.txt.sigstore.json $V/cosign-v3.0.1/cosign_checksums.txt $ID
done
