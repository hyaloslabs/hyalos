#!/usr/bin/env bash
# Model-checks the job state machine (S8.2, ADR-0108): spec/Job.tla under spec/Job.cfg must hold every
# invariant, and each mutant must break the one invariant it is for, so no invariant passes by checking
# nothing. Usage: check-spec.sh <tla2tools.jar>. Needs java on PATH.
set -euo pipefail

jar="$(realpath "${1:?usage: check-spec.sh <tla2tools.jar>}")"
spec="$(cd "$(dirname "$0")/../../spec" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

tlc() {
  # -deadlock: the log's bound leaves states with no successor, which are not deadlocks of the system.
  java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto -deadlock -cleanup \
    -metadir "$1/states" -config "$1/Job.cfg" "$1/Job.tla"
}

echo "== the model, Mutant = \"none\""
mkdir -p "$work/none"
cp "$spec/Job.tla" "$spec/Job.cfg" "$work/none/"
tlc "$work/none" | tee "$work/none.out"
grep -q "Model checking completed. No error has been found." "$work/none.out"

# Each mutant and the invariant it must break.
failed=0
for pair in \
  "split_hold HeldTaskRunsNoJob" \
  "stale_merge MergeOnlyAtApprovedHead" \
  "late_work ExpiredManifestAuthorizesNothing" \
  "unlogged_release ProjectionIsReplay"; do
  read -r mutant invariant <<<"$pair"
  dir="$work/$mutant"
  mkdir -p "$dir"
  cp "$spec/Job.tla" "$dir/"
  sed "s/Mutant = \"none\"/Mutant = \"$mutant\"/" "$spec/Job.cfg" > "$dir/Job.cfg"
  grep -q "Mutant = \"$mutant\"" "$dir/Job.cfg"
  echo "== Mutant = \"$mutant\" must break $invariant"
  if tlc "$dir" > "$dir.out" 2>&1; then
    echo "::error::the $mutant mutant broke no invariant"
    failed=1
  elif grep -q "Invariant $invariant is violated" "$dir.out"; then
    echo "broken, as it must be"
  else
    echo "::error::the $mutant mutant did not break $invariant"
    tail -n 40 "$dir.out"
    failed=1
  fi
done
exit "$failed"
