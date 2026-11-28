#!/usr/bin/env bash
# Measures each runtime that built on the issue fixture, and lists what each hostile case gave
# (Slice 5 of docs/plans/hyalos-kernel-python, S-06.2). It prints the two tables of
# MEASUREMENTS.md, in its form.
#
#   spikes/438/measure.sh            print the tables
#   spikes/438/measure.sh --check    stop unless MEASUREMENTS.md's tables hold the same figures
#
# The issue fixture table has one row per runtime and backend: the guest's size, the cold start and
# run time `spike-run` prints on its `timing:` line (each the median of three runs), and the fuel
# its `ok:` line prints, with the command that measured them. A runtime or backend README.md names
# on a `process-only:` line gives that reason in place of figures, and a runtime that did not build
# gives why. The hostile table has one row per hostile case and runtime in expected.tsv, from its
# committed record: the outcome, the labels asked, the notification body's SHA-256, the result and
# the fuel (for `OutOfFuel`, the profile's, which the run used up).
#
# `--check` measures again and compares every figure but the times, which vary from run to run
# and host to host, so they are recorded, not reproduced; it also checks that every row with a
# time names its command. Needs cargo and python3.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

fail() { echo "measure.sh: $*" >&2; exit 1; }

check=false
case "${1:-}" in
  "") ;;
  --check) check=true ;;
  *) fail "usage: measure.sh [--check]" ;;
esac

target="$root/hyalos-os/target/spike"
cargo build -q --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel \
  --features spike-contract --target-dir "$target" >&2
bin=hyalos-os/target/spike/release/task-kernel
readme=spikes/438/README.md
fixture=hyalos-os/evidence/fixtures/issues.tsv
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

profile_of() { sed -n "s/^$1 profile: \([^ ]*\).*/\1/p" "$readme" | head -1; }
process_only() { sed -n "s/^process-only: $1 — \(.*\)/\1/p" "$readme" | head -1; }

# One spike-run of the issue fixture at seed 0: its `ok:` and `timing:` lines. The whole output is
# captured first and its first two lines taken after: a `| head -2` could close the pipe while
# spike-run still writes, which ends it on a broken pipe and, under pipefail, fails the script.
run_once() {
  local profile=$1 guest=$2 backend=$3 image=${4:-} out
  if [ "$backend" = kvm ]; then
    out=$(HYALOS_SPIKE_PROFILE=$profile HYALOS_KERNEL="$image" "$bin" spike-run "$guest" \
      spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py "$fixture" \
      "$tmp/out.json" 0)
  else
    out=$(HYALOS_SPIKE_PROFILE=$profile env -u HYALOS_KERNEL "$bin" spike-run "$guest" \
      spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py "$fixture" \
      "$tmp/out.json" 0)
  fi
  sed -n 1,2p <<<"$out"
}

table=$tmp/table.md
{
  echo "| Runtime | Backend | Guest bytes | Cold start (ms) | Run time (ms) | Fuel | Command |"
  echo "|---|---|---|---|---|---|---|"
  for runtime in micropython monty; do
    guest=spikes/438/guests/$runtime.wasm
    profile=$(profile_of "$runtime")
    if [ ! -f "$guest" ] || [ -z "$profile" ]; then
      for backend in process kvm; do
        echo "| $runtime | $backend | — | — | — | — | did not build: no guest under spikes/438/guests with a \`$runtime profile:\` line |"
      done
      continue
    fi
    size=$(stat -c %s "$guest")
    for backend in process kvm; do
      reason=
      if [ "$backend" = kvm ]; then
        reason=$(process_only "$runtime")
        image=
        [ -n "$reason" ] || image=$(spikes/438/kernel.sh "$profile")
      fi
      if [ -n "$reason" ]; then
        echo "| $runtime | kvm | — | — | — | — | process-only: $reason |"
        continue
      fi
      for i in 1 2 3; do
        out=$(run_once "$profile" "$guest" "$backend" "${image:-}")
        case "$out" in ok:*) ;; *) fail "$runtime on $backend: '$out'" ;; esac
        echo "$out" > "$tmp/run-$i"
      done
      cmd="HYALOS_SPIKE_PROFILE=$profile"
      [ "$backend" = process ] || cmd="$cmd HYALOS_KERNEL=\$(spikes/438/kernel.sh $profile)"
      cmd="$cmd $bin spike-run $guest spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py $fixture out.json 0"
      python3 -I - "$runtime" "$backend" "$size" "$cmd" "$tmp"/run-1 "$tmp"/run-2 "$tmp"/run-3 <<'PY'
import re, statistics, sys
runtime, backend, size, cmd, *runs = sys.argv[1:]
fuel, cold, total = set(), [], []
for path in runs:
    text = open(path).read()
    fuel.add(int(re.search(r"fuel (\d+)", text).group(1)))
    m = re.search(r"cold_start ([\d.]+) ms, run_time ([\d.]+) ms", text)
    cold.append(float(m.group(1)))
    total.append(float(m.group(2)))
if len(fuel) != 1:
    sys.exit("measure.sh: %s on %s: the fuel differs between runs" % (runtime, backend))
print("| %s | %s | %s | %.1f | %.1f | %s | `%s` |" % (
    runtime, backend, format(int(size), ","), statistics.median(cold),
    statistics.median(total), format(fuel.pop(), ","), cmd))
PY
    done
  done
  echo
  echo "| Case | Runtime | Outcome | Asked (bug, feature, security, other) | Notification SHA-256 | Result | Fuel |"
  echo "|---|---|---|---|---|---|---|"
  python3 -I - spikes/438/evidence/expected.tsv "$readme" <<'PY'
import hashlib, json, re, sys
expected, readme = sys.argv[1:]
profiles = dict(re.findall(r"^(\w+) profile: (\S+)", open(readme).read(), re.M))
# Each profile's fuel (`hyalos-os/profile/src/lib.rs`), as spikes/438/hostile.sh has it.
fuel = {"hyalos.spike/1": 200000000, "hyalos.spike/2": 830000000}
for line in open(expected):
    if line.startswith("#") or "-hostile-" not in line.split("\t")[0]:
        continue
    f = line.rstrip("\n").split("\t")
    name, guest, outcome, record = f[0], f[1], f[8], f[9]
    runtime, case = name.split("-hostile-")
    if record == "-":
        spent = fuel[profiles[runtime]] if outcome == "refused OutOfFuel" else None
        print("| %s | %s | %s | — | — | — | %s |" % (
            case, runtime, outcome, format(spent, ",") if spent else "—"))
        continue
    t = json.load(open(record))
    asked = [a["label"] for a in t["answers"]]
    counts = ", ".join(str(asked.count(i)) for i in range(4))
    body = (hashlib.sha256(bytes.fromhex(t["effects"][0]["body_hex"])).hexdigest()[:16]
            if t["effects"] else "none")
    result = bytes.fromhex(t["result_hex"]).decode("utf-8", "replace")
    print("| %s | %s | %s | %d (%s) | `%s` | `%s` | %s |" % (
        case, runtime, outcome, len(asked), counts, body, result, format(t["fuel_used"], ",")))
PY
} > "$table"

if ! $check; then
  cat "$table"
  exit 0
fi

# Every figure but the two time columns, from a table's rows.
python3 -I - "$table" spikes/438/MEASUREMENTS.md <<'PY'
import sys
def rows(path):
    out = []
    for line in open(path, encoding="utf-8"):
        if not line.startswith("| ") or line.startswith("|---") or line.startswith("| Runtime ") \
                or line.startswith("| Case "):
            continue
        out.append([c.strip() for c in line.strip().strip("|").split("|")])
    return out
made, held = rows(sys.argv[1]), rows(sys.argv[2])
def figures(r):
    # An issue fixture row has seven cells and its times at 3 and 4.
    return [c for i, c in enumerate(r) if not (len(r) == 7 and r[1] in ("process", "kvm") and i in (3, 4))]
bad = []
if len(made) != len(held):
    bad.append("%d rows measured, %d in MEASUREMENTS.md" % (len(made), len(held)))
for m, h in zip(made, held):
    if figures(m) != figures(h):
        diff = [(a, b) for a, b in zip(figures(m), figures(h)) if a != b]
        bad.append("%s %s: measured %s, MEASUREMENTS.md has %s"
                   % (m[0], m[1], [a for a, _ in diff], [b for _, b in diff]))
for h in held:
    if len(h) == 7 and h[1] in ("process", "kvm") and h[3] != "—" and not h[6].startswith("`"):
        bad.append("%s on %s: a time with no command" % (h[0], h[1]))
if bad:
    sys.exit("measure.sh --check:\n  " + "\n  ".join(bad))
print("measure: %d rows match MEASUREMENTS.md (times not compared)" % len(held))
PY
