#!/usr/bin/env bash
# Whether this commit's verifier rebuilds to the same bytes in two directories, and whether a source
# with one changed byte gives different bytes (ADR-0131). Each build starts clean. It publishes nothing.
set -euo pipefail

commit="$(git rev-parse HEAD)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

for dir in first elsewhere/second altered; do
  git clone -q "$PWD" "$scratch/$dir" 2>/dev/null
  test "$(git -C "$scratch/$dir" rev-parse HEAD)" = "$commit"
done
sed -i 's/"refused: {what}"/"Refused: {what}"/' "$scratch/altered/hyalos-os/verify/src/main.rs"
test "$(git -C "$scratch/altered" diff --numstat | wc -l)" = 1

pids=()
for dir in first elsewhere/second altered; do
  (cd "$scratch/$dir" && .github/scripts/build-verifier.sh dist) > "$scratch/$(basename "$dir").log" 2>&1 &
  pids+=("$!")
done
failed=0
for pid in "${pids[@]}"; do
  wait "$pid" || failed=1
done
if [ "$failed" -ne 0 ]; then
  tail -n 40 "$scratch"/*.log
  exit 1
fi

hash() { cut -d' ' -f1 "$scratch/$1/dist/SHA256SUMS"; }
first="$(hash first)" second="$(hash elsewhere/second)" altered="$(hash altered)"
echo "first:   $first"
echo "second:  $second"
echo "altered: $altered"
grep -qxF "commit $commit" "$scratch/first.log" || { echo "the build does not name $commit"; exit 1; }
[[ "$first" =~ ^[0-9a-f]{64}$ ]] || { echo "no hash"; exit 1; }
[ "$first" = "$second" ] || { echo "two builds of $commit differ"; exit 1; }
[ "$first" != "$altered" ] || { echo "a changed source gave the same binary"; exit 1; }
echo "two builds of $commit are identical, and a changed byte changes the hash"
