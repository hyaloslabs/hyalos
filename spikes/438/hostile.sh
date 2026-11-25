#!/usr/bin/env bash
# Runs the triage task on every hostile input in fixtures/hostile.tsv under each runtime that built,
# and writes what each run gave (Slice 5 of docs/plans/hyalos-kernel-python, S-06.1).
#
#   spikes/438/hostile.sh
#
# A runtime that built is a guest under guests/ with a `<runtime> profile: <name>` line in
# README.md. For each case and runtime, the script decodes the case's lines into a data file,
# evidence/data/hostile-<case>.tsv for a case every line of which is tagged `all` and
# evidence/data/<runtime>-hostile-<case>.tsv otherwise, and runs it at seed 0, with the draft contract, its
# SDK and scripts/triage.py, on the process backend, and on KVM unless README.md has a
# `process-only:` line for the runtime or the case. A run that ends with a result writes its record
# to evidence/<runtime>-hostile-<case>.json; a refused run writes none.
#
# It prints, per case and runtime:
#   - each leg's outcome in expected.tsv's form, and the line for expected.tsv, which the executor
#     copies in by hand once reviewed (this script never writes expected.tsv). A case already
#     listed there must give the outcome it lists, or the script stops;
#   - its reach: the labels it asked, the notification body's SHA-256, the result and the fuel.
#     For a run that ends `OutOfFuel` the fuel is the profile's, which it used up.
# It ends with "hostile: ok".
#
# It stops, as the slice's stop conditions say, if a run crashes (no `ok:` or `refused:` line), if a
# record holds a question of another schema or an effect of another kind or more than one, or if
# the two backends' records differ. Needs cargo and python3.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

fail() { echo "hostile.sh: $*" >&2; exit 1; }

fixture=spikes/438/fixtures/hostile.tsv
evidence=spikes/438/evidence
mkdir -p "$evidence/data"

# The colliding blocks are the ones collide.py's hashes give.
for runtime in micropython monty; do
  python3 -I spikes/438/fixtures/collide.py --check "$runtime" "$fixture"
done

target="$root/hyalos-os/target/spike"
cargo build -q --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel \
  --features spike-contract --target-dir "$target" >&2
kernel_bin="$target/release/task-kernel"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

readme=spikes/438/README.md
profile_of() { sed -n "s/^$1 profile: \([^ ]*\).*/\1/p" "$readme" | head -1; }
process_only() { grep -q "^process-only: $1 — " "$readme"; }

# Each profile's fuel (`hyalos-os/profile/src/lib.rs`: `hyalos.spike/1` is profile 2's,
# `hyalos.spike/2` its own), the figure a run that ends `OutOfFuel` used.
fuel_of() {
  case "$1" in
    hyalos.spike/1) echo 200000000 ;;
    hyalos.spike/2) echo 830000000 ;;
    *) fail "no fuel figure for profile $1" ;;
  esac
}

# Decodes the lines of one case for one runtime (a line tagged `all` or the runtime) into a data
# file, one record per line.
decode() {
  python3 -I - "$fixture" "$1" "$2" "$3" <<'PY'
import sys
fixture, case, runtime, out = sys.argv[1:]
data = bytearray()
for line in open(fixture, encoding="ascii"):
    line = line.rstrip("\n")
    if not line or line.startswith("#"):
        continue
    fields = line.split("\t")
    if len(fields) != 3:
        sys.exit("hostile.tsv: a line without three fields: %r" % line[:60])
    if fields[0] != case or fields[1] not in ("all", runtime):
        continue
    text, i, one = fields[2], 0, bytearray()
    while i < len(text):
        if text[i] == "\\":
            if text[i + 1:i + 2] != "x" or len(text) < i + 4:
                sys.exit("hostile.tsv: %s: a backslash that is not \\xNN" % case)
            one.append(int(text[i + 2:i + 4], 16))
            i += 4
        else:
            one.extend(text[i].encode())
            i += 1
    # The loader splits a data file on newlines and drops empty lines, so either would change
    # what the case's records are.
    if not one or b"\n" in one:
        sys.exit("hostile.tsv: %s: an empty data line, or one holding a newline" % case)
    data += one + b"\n"
if not data:
    sys.exit("hostile.tsv: %s has no line for %s" % (case, runtime))
open(out, "wb").write(bytes(data))
PY
}

# The case ids in the order they first appear, with the runtimes each applies to.
cases=$(awk -F'\t' '!/^#/ && NF == 3 && !seen[$1 FS $2]++ { print $1 "\t" $2 }' "$fixture")

runs=0
while IFS= read -r guest; do
  runtime=$(basename "$guest" .wasm)
  profile=$(profile_of "$runtime")
  [ -n "$profile" ] || { echo "$runtime: no '$runtime profile:' line, so it did not build"; continue; }
  fuel=$(fuel_of "$profile")
  image=
  for id in $(printf '%s\n' "$cases" | awk -F'\t' -v r="$runtime" '$2 == "all" || $2 == r { print $1 }' | awk '!seen[$0]++'); do
    name="$runtime-hostile-$id"
    # A case whose lines are all tagged `all` has one data file for every runtime.
    if printf '%s\n' "$cases" | awk -F'\t' -v id="$id" '$1 == id && $2 != "all" { f = 1 } END { exit !f }'; then
      data="$evidence/data/$name.tsv"
    else
      data="$evidence/data/hostile-$id.tsv"
    fi
    record="$evidence/$name.json"
    decode "$id" "$runtime" "$data"
    legs=(process)
    if process_only "$runtime" || process_only "$name"; then
      kvm_reason=$(sed -n "s/^process-only: \($runtime\|$name\) — \(.*\)/\2/p" "$readme" | head -1)
    else
      legs+=(kvm)
      [ -n "$image" ] || image=$(spikes/438/kernel.sh "$profile")
    fi
    outcome= first=
    for leg in "${legs[@]}"; do
      out="$tmp/$name-$leg.json"
      if [ "$leg" = kvm ]; then
        line=$(HYALOS_SPIKE_PROFILE=$profile HYALOS_KERNEL="$image" "$kernel_bin" spike-run \
          "$guest" spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py \
          "$data" "$out" 0 2>&1 | head -1 || true)
      else
        line=$(HYALOS_SPIKE_PROFILE=$profile env -u HYALOS_KERNEL "$kernel_bin" spike-run \
          "$guest" spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py \
          "$data" "$out" 0 2>&1 | head -1 || true)
      fi
      case "$line" in
        ok:*)
          got=$(python3 -I - "$out" "$profile" <<'PY'
import json, sys
t = json.load(open(sys.argv[1]))
assert t["profile"] == sys.argv[2], t["profile"]
if any(a.get("schema", 0) != 0 for a in t["answers"]) or len(t["effects"]) > 1 \
        or any(e.get("kind", 0) != 0 for e in t["effects"]):
    sys.exit("an ungranted schema or effect is in the record")
print("result", bytes.fromhex(t["result_hex"]).decode(), "effects", len(t["effects"]))
PY
          ) || fail "$name on $leg: the record breaks its contract (a monitor bug: stop)" ;;
        refused:*) [ ! -e "$out" ] || fail "$name on $leg: a refused run wrote a record"
          got="refused ${line#refused: }" ;;
        *) fail "$name on $leg: no outcome ('$line'): a crash, which is a stop condition" ;;
      esac
      echo "$name $leg: $got"
      if [ -z "$outcome" ]; then
        outcome=$got first=$out
      else
        [ "$got" = "$outcome" ] || fail "$name: the backends differ: '$outcome' and '$got'"
        [ ! -e "$out" ] || cmp -s "$out" "$first" || fail "$name: the backends' records differ"
      fi
    done
    [ "${#legs[@]}" -eq 2 ] || echo "$name kvm: process-only — $kvm_reason"
    case "$outcome" in
      result*) cp "$first" "$record" ;;
      *) rm -f "$record" ;;
    esac

    # Its reach: the labels asked, the notification body's hash, the result and the fuel.
    python3 -I - "$record" "$outcome" "$fuel" "$name" <<'PY'
import hashlib, json, os, sys
record, outcome, fuel, name = sys.argv[1:]
labels = ("bug", "feature", "security", "other")
if outcome.startswith("result"):
    t = json.load(open(record))
    asked = [a["label"] for a in t["answers"]]
    counts = " ".join("%s %d" % (l, asked.count(i)) for i, l in enumerate(labels))
    body = (hashlib.sha256(bytes.fromhex(t["effects"][0]["body_hex"])).hexdigest()[:16]
            if t["effects"] else "none")
    result = bytes.fromhex(t["result_hex"]).decode("utf-8", "replace")
    print("reach %s: asked %d (%s), notification %s, result %r, fuel %d"
          % (name, len(asked), counts, body, result, t["fuel_used"]))
else:
    spent = fuel if outcome == "refused OutOfFuel" else "-"
    print("reach %s: %s, no record, so nothing asked or sent is known; fuel %s"
          % (name, outcome, spent))
PY

    backends=process
    [ "${#legs[@]}" -eq 1 ] || backends=both
    rec=-
    case "$outcome" in result*) rec=$record ;; esac
    want=$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' "$name" "$guest" spikes/438/contracts/draft \
      spikes/438/sdk/draft.py spikes/438/scripts/triage.py "$data" 0 "$backends" "$outcome" "$rec")
    listed=$(awk -F'\t' -v id="$name" '$1 == id' "$evidence/expected.tsv")
    if [ -z "$listed" ]; then
      echo "not yet in expected.tsv: $want"
    else
      [ "$listed" = "$want" ] || fail "$name: expected.tsv lists '$listed', the run gave '$want'"
    fi
    runs=$((runs + 1))
  done
done < <(ls spikes/438/guests/*.wasm)

[ "$runs" -gt 0 ] || fail "no runtime built, so no case ran"
echo "hostile: ok"
