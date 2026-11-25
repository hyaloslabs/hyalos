#!/usr/bin/env bash
# Checks the SDK generator and the triage script (Slice 2 of docs/plans/hyalos-kernel-python):
#   S-02.1.e1, 2.1  every SDK generates the same bytes twice, and the committed ones are those bytes
#   S-02.1.e2       a reordered or whitespace-padded contract is refused
#   S-02.2.e1, S-02.3.e1, S-02.3.e2  each SDK's public names are exactly design.md's
#   2.2             triage.py imports only `task` from `hyalos` and names nothing but its own
#                   variables, `task` and builtins
#   2.3             triage.py with the draft SDK and an all-other fake `_hyalos` gives the digest
#                   format on the issue fixture
# It ends `check: ok`, or names what failed and exits non-zero. Needs Python 3 on the host only.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
py() { python3 -I "$@"; }
fail() { echo "check: FAIL: $*" >&2; exit 1; }

# S-02.1.e1 and 2.1: deterministic, and committed as generated.
for c in draft no-notify no-ask; do
  py "$here/generate/generate.py" "$here/contracts/$c" > "$work/$c.1.py"
  py "$here/generate/generate.py" "$here/contracts/$c" > "$work/$c.2.py"
  cmp -s "$work/$c.1.py" "$work/$c.2.py" || fail "$c: two generations differ"
  cmp -s "$work/$c.1.py" "$here/sdk/$c.py" || fail "$c: sdk/$c.py is not what the generator writes"
  echo "generated twice, identical and committed: $c ($(sha256sum < "$here/sdk/$c.py" | cut -c1-16))"
done

# S-02.1.e2: only the canonical bytes are a contract.
printf '%s' '{"may_touch":[0],"asks":[0]}' > "$work/reordered"
printf '%s' '{"asks": [0],"may_touch":[0]}' > "$work/padded"
printf '%s\n' '{"asks":[0],"may_touch":[0]}' > "$work/newline"
for bad in reordered padded newline; do
  if py "$here/generate/generate.py" "$work/$bad" > "$work/out" 2> "$work/err"; then
    fail "the $bad contract was not refused"
  fi
  [ ! -s "$work/out" ] || fail "the $bad contract wrote output"
  echo "refused: $bad ($(cat "$work/err"))"
done

# S-02.2.e1, S-02.3.e1, S-02.3.e2: the public names, with a `_hyalos` that does nothing.
names() {
  py - "$1" <<'PY'
import sys, types
sys.modules["_hyalos"] = types.ModuleType("_hyalos")
module = types.ModuleType("hyalos")
exec(compile(open(sys.argv[1], "rb").read(), "hyalos", "exec"), module.__dict__)
public = lambda o: sorted(n for n in dir(o) if not n.startswith("_"))
mod = [n for n in public(module) if n != "__builtins__"]
task = module.task
members = []
for n in public(task):
    value = getattr(task, n)
    if n in ("ask", "notify"):
        members += [n + "." + m for m in public(value)]
    else:
        members.append(n)
print("module: " + " ".join(mod) + "; task: " + " ".join(members))
PY
}
expect() {
  got=$(names "$here/sdk/$1.py")
  [ "$got" = "$2" ] || fail "$1: public names are '$got', not '$2'"
  echo "names: $1: $got"
}
expect draft "module: task; task: ask.classify finish notify.team read_inputs"
expect no-notify "module: task; task: ask.classify finish read_inputs"
expect no-ask "module: task; task: finish notify.team read_inputs"

# 2.2: the script's imports and names.
py - "$here/scripts/triage.py" <<'PY' || fail "triage.py uses a name outside the SDK and builtins"
import ast, builtins, sys
tree = ast.parse(open(sys.argv[1], "rb").read())
imports = [n for n in ast.walk(tree) if isinstance(n, (ast.Import, ast.ImportFrom))]
shape = [(n.module, [a.name for a in n.names], [a.asname for a in n.names]) for n in imports
         if isinstance(n, ast.ImportFrom)]
assert len(imports) == 1 and shape == [("hyalos", ["task"], [None])], ast.dump(imports[0])
own = {n.id for n in ast.walk(tree) if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Store)}
used = {n.id for n in ast.walk(tree) if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Load)}
outside = used - own - {"task"} - set(dir(builtins))
assert not outside, outside
assert not any(isinstance(n, (ast.Global, ast.Nonlocal, ast.Lambda)) for n in ast.walk(tree))
print("triage.py: imports only `from hyalos import task`; other names: its own and builtins")
PY

# 2.3: the digest format, on the issue fixture, every answer other.
fixture="$root/hyalos-os/evidence/fixtures/issues.tsv"
n=$(grep -c . "$fixture")
py "$here/generate/fake_hyalos.py" "$here/sdk/draft.py" "$here/scripts/triage.py" "$fixture" > "$work/run.json"
py - "$work/run.json" "$n" <<'PY' || fail "triage.py under the fake does not give the digest format"
import json, sys
run, n = json.load(open(sys.argv[1])), int(sys.argv[2])
assert run["effects"] == [[0, f"total {n} bug 0 feature 0 security 0 other {n}\n"]], run["effects"]
assert run["result"] == f"ok {n}", run["result"]
assert len(run["questions"]) == n and all(q[0] == 0 for q in run["questions"])
print(f"2.3: notification 'total {n} bug 0 feature 0 security 0 other {n}\\n', result 'ok {n}'")
PY

# The no-notify SDK under the same script: the ungranted member is an attribute error in Python,
# after the questions it asked (what S-04.2.e1 then checks inside a runtime guest).
py "$here/generate/fake_hyalos.py" "$here/sdk/no-notify.py" "$here/scripts/triage.py" "$fixture" > "$work/no-notify.json"
py - "$work/no-notify.json" "$n" <<'PY' || fail "triage.py under the no-notify SDK did not fail on notify"
import json, sys
run, n = json.load(open(sys.argv[1])), int(sys.argv[2])
assert run["effects"] == [] and run["result"] == "error AttributeError", run
assert len(run["questions"]) == n
print("no-notify: AttributeError after", n, "questions, no effect")
PY

echo "check: ok"
