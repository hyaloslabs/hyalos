#!/usr/bin/env bash
# Runs the MicroPython cases of spikes/438/evidence/expected.tsv and writes their records (Slice 3
# of docs/plans/hyalos-kernel-python).
#
#   spikes/438/micropython/run.sh
#
# It reads the cases from expected.tsv, which is written by hand and which this script never
# writes. Each case runs on every backend its line lists: a result case writes its record to the
# file the line names, and a refused case writes none. The script stops unless every outcome is the
# one the line gives. It ends with "micropython: ok".
#
# MicroPython is listed `both`: the default kernel image runs it since it builds `wasmi` with
# `portable-dispatch` (issue #503, Slice 1 of docs/plans/hyalos-kernel-capsules). After the cases,
# the script runs the triage case once more on KVM and stops if it ends `KernelFault` there, so a
# return of the stack overflow is noticed. With `HYALOS_REQUIRE_KVM` set, a host without KVM fails.
#
# Needs cargo and python3. The spike's task-kernel build goes to its own target directory.
set -euo pipefail

root=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$root"

fail() { echo "run.sh: $*" >&2; exit 1; }

target="$root/hyalos-os/target/spike"
cargo build -q --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel \
  --features spike-contract --target-dir "$target" >&2
kernel_bin="$target/release/task-kernel"
image=$(spikes/438/kernel.sh hyalos.spike/1)

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The outcome of one run, in expected.tsv's form, from its output and the record it wrote.
outcome() {
  local output=$1 record=$2
  case "$output" in
    ok:*)
      python3 -I -c '
import json, sys
t = json.load(open(sys.argv[1]))
print("result", bytes.fromhex(t["result_hex"]).decode(), "effects", len(t["effects"]))
' "$record" ;;
    refused:*) [ ! -e "$record" ] || fail "a refused run wrote $record"
      echo "refused ${output#refused: }" ;;
    *) echo "unknown: $output" ;;
  esac
}

# One run on one backend: `process`, or `kvm` with the default kernel image.
spike_run() {
  local backend=$1; shift
  if [ "$backend" = kvm ]; then
    HYALOS_KERNEL="$image" "$kernel_bin" spike-run "$@" 2>&1 | head -1 || true
  else
    env -u HYALOS_KERNEL "$kernel_bin" spike-run "$@" 2>&1 | head -1 || true
  fi
}

cases=0
while IFS=$'\t' read -r id guest contract sdk script data seed backends expected record; do
  case "$id" in micropython-*) ;; *) continue ;; esac
  case "$id" in *-hostile-*) continue ;; esac
  case "$backends" in process) legs=(process) ;; both) legs=(process kvm) ;;
    *) fail "$id: backends '$backends'" ;; esac
  for leg in "${legs[@]}"; do
    out="$tmp/$id-$leg.json"
    line=$(spike_run "$leg" "$guest" "$contract" "$sdk" "$script" "$data" "$out" "$seed")
    got=$(outcome "$line" "$out")
    [ "$got" = "$expected" ] || fail "$id on $leg: '$got', expected '$expected'"
    if [ "$record" != - ]; then
      if [ "$leg" = process ]; then
        cp "$out" "$record"
      else
        cmp -s "$out" "$record" || fail "$id: the backends' records differ"
      fi
    fi
    echo "$id $leg: $got"
  done
  cases=$((cases + 1))
done < spikes/438/evidence/expected.tsv
[ "$cases" -gt 0 ] || fail "expected.tsv lists no MicroPython case"

if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
  line=$(spike_run kvm spikes/438/guests/micropython.wasm spikes/438/contracts/draft \
    spikes/438/sdk/draft.py spikes/438/scripts/triage.py hyalos-os/evidence/fixtures/issues.tsv \
    "$tmp/kvm.json" 0)
  [ "$line" != "refused: KernelFault" ] \
    || fail "on KVM the triage case ended KernelFault: the kernel stack overflow of issue #503 is back"
  got=$(outcome "$line" "$tmp/kvm.json")
  want=$(awk -F'\t' '$1 == "micropython-triage-0" { print $9 }' spikes/438/evidence/expected.tsv)
  [ "$got" = "$want" ] || fail "on KVM the triage case gave '$got', expected '$want'"
  echo "micropython-triage-0 kvm: $got"
else
  echo "micropython: no KVM here, so the KVM check did not run"
  [ -z "${HYALOS_REQUIRE_KVM:-}" ] || fail "HYALOS_REQUIRE_KVM is set and /dev/kvm cannot be opened"
fi
echo "micropython: ok"
