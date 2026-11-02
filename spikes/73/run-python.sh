#!/bin/bash
# Runs every verification in a container with no network, using sigstore-python.
#   ./run-python.sh            the main matrix against the shipped snapshot
#   ./run-python.sh history    also fetch old trusted roots (needs a network) and try each against both bundles
cd "$(dirname "$0")"
ID=keyless@projectsigstore.iam.gserviceaccount.com
ISS=https://accounts.google.com
docker build -q -t hyalos-spike73 . >/dev/null || exit 1
python3 variants.py "$@" || exit 1

verify() {  # label  trust-config  bundle  artifact  identity
  local out rc r
  out=$(docker run --rm --network none -v "$PWD":/w:ro -w /w hyalos-spike73 \
    sigstore --trust-config "$2" verify identity --offline --bundle "$3" \
    --cert-identity "$5" --cert-oidc-issuer "$ISS" "$4" 2>&1); rc=$?
  if [ $rc -eq 0 ]; then r="accepted"; else
    r="refused: $(echo "$out" | grep ERROR | grep -v 'FAIL:' | head -1 | sed 's/.*ERROR *//; s/ *errors.py.*//' | cut -c1-64)"; fi
  printf '  %-42s %s\n' "$1" "$r"
}

V=vectors; CUR=run/cfg/current.json
echo "container has no network: $(docker run --rm --network none hyalos-spike73 python -c \
  "import socket
try: socket.create_connection(('tuf-repo-cdn.sigstore.dev',443),3)
except Exception as e: print('yes (',type(e).__name__,')')")"
echo "== main matrix (current snapshot)"
verify "fresh bundle (2026-10-20)"      $CUR $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json $V/cosign-v3.1.4/cosign_checksums.txt $ID
verify "year-old bundle (2025-10-08)"   $CUR $V/cosign-v3.0.1/cosign_checksums.txt.sigstore.json $V/cosign-v3.0.1/cosign_checksums.txt $ID
verify "tampered artifact"              $CUR $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json run/tamper/cosign_checksums.txt $ID
verify "wrong identity"                 $CUR $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json $V/cosign-v3.1.4/cosign_checksums.txt attacker@example.com
echo "== evidence removed from the fresh bundle"
for f in run/strip/*.sigstore.json; do
  verify "$(basename "$f" .sigstore.json)" $CUR "$f" $V/cosign-v3.1.4/cosign_checksums.txt $ID
done
if [ "$1" = history ]; then
  echo "== old trusted roots (rows) against each bundle"
  for c in $(ls run/hist/*.cfg.json | sort -V); do
    n=$(basename "$c" .cfg.json)
    verify "$n / fresh"      "$c" $V/cosign-v3.1.4/cosign_checksums.txt.sigstore.json $V/cosign-v3.1.4/cosign_checksums.txt $ID
    verify "$n / year-old"   "$c" $V/cosign-v3.0.1/cosign_checksums.txt.sigstore.json $V/cosign-v3.0.1/cosign_checksums.txt $ID
  done
fi
