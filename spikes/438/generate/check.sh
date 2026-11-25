#!/usr/bin/env bash
# Checks the SDK generator and the triage script (Slice 2 of docs/plans/hyalos-kernel-python):
#   S-02.1.e1, 2.1  every SDK generates the same bytes twice, and the committed ones are those bytes
#   S-02.1.e2       a reordered, whitespace-padded or newline-terminated contract is refused
#   S-02.2.e1, S-02.3.e1, S-02.3.e2  each SDK's public names are exactly design.md's
#   2.2             triage.py imports only `task` from `hyalos` and names nothing but its own
#                   variables, `task` and builtins
#   2.3             triage.py with the draft SDK and an all-other fake `_hyalos` gives the digest
#                   format on the issue fixture
#   2.2             with every answer security, on the fixture and on inputs at the digest guest's
#                   limits, its notification and result are those of a line-by-line port of the
#                   digest guest (hyalos-os/guest/src/lib.rs)
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
mod = public(module)
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
# Builtins that reach outside the script: none may be named at all.
reach = {"__import__", "exec", "eval", "compile", "open", "getattr", "setattr", "delattr",
         "globals", "locals", "vars", "input", "breakpoint", "help", "memoryview", "__build_class__"}
assert not used & reach, used & reach
assert not any(isinstance(n, (ast.Global, ast.Nonlocal, ast.Lambda)) for n in ast.walk(tree))
attrs = {n.attr for n in ast.walk(tree) if isinstance(n, ast.Attribute)}
assert not any(a.startswith("_") for a in attrs), attrs
print("triage.py: imports only `from hyalos import task`; other names: its own and builtins,")
print("  none of", len(reach), "that reach outside the script, and no attribute starting _")
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

# 2.2: every answer security, against a port of the digest guest, on the fixture and on inputs at
# its limits: titles of 4,095 to 4,097 bytes, a record over 16,384 bytes, numbers with leading
# zeros, at and over u32's range, signs and spaces, a record with no tab and one with an empty
# title, 210 records, and security lines landing exactly on the 8,000-byte limit.
py - "$work" "$fixture" <<'PY'
import sys
work, fixture = sys.argv[1:3]
fixture_lines = [l for l in open(fixture, "rb").read().split(b"\n") if l]
edge = [b"7\t" + b"a" * 4095, b"8\t" + b"b" * 4096, b"9\t" + b"c" * 4097, b"10\t" + b"d" * 16380,
        b"007\tzeros", b"4294967295\tmax", b"4294967296\tover", b"-1\tsign", b"+1\tsign",
        b" 1\tspace", b"no tab here", b"11\t", b"12a\tletters"]
edge += [str(100 + i).encode() + b"\tt" + str(i).encode() for i in range(200)]
cases = {"fixture": fixture_lines, "edge": edge}
# Digests landing exactly on the 8,000-byte limit and one byte past it, with the long line last
# (kept whole when it fits) and not last (kept only with room for the truncation line). Each
# security line is "#1nn " (5 bytes), its title and a newline; titles stay under 4,096 bytes.
def header(n):
    return len(b"total %d bug 0 feature 0 security %d other 0\n" % (n, n))
for extra, tag in ((0, "exact"), (1, "over")):
    second = 8000 - header(2) - (3900 + 6) - 6 + extra
    cases[tag + "-last"] = [b"100\t" + b"x" * 3900, b"101\t" + b"y" * second]
    second = 8000 - header(3) - (3900 + 6) - 6 - len(b"...truncated\n") + extra
    cases[tag + "-not-last"] = [b"100\t" + b"x" * 3900, b"101\t" + b"y" * second,
                                b"102\t" + b"z" * 100]
for name, lines in cases.items():
    open(f"{work}/{name}.tsv", "wb").write(b"\n".join(lines) + b"\n")
print(" ".join(cases))
PY
digest_port() {
  py - "$1" <<'PY'
# hyalos-os/guest/src/lib.rs, line by line, with every answer 2 (security). Records 0 to 2 are the
# spike's contract, SDK and script, which a runtime guest does not read as issues.
import json, sys
MAX_RECORDS, REC_CAP, TITLE_MAX, DIGEST_MAX = 200, 16384, 4096, 8000
TRUNC_LINE = b"...truncated\n"
records = [l for l in open(sys.argv[1], "rb").read().split(b"\n") if l]
def parse_number(s):
    if not s or any(c < 48 or c > 57 for c in s):
        return None
    n = int(s)
    return n if n <= 0xFFFFFFFF else None
counts, total, sec = [0, 0, 0, 0], 0, []
for index in range(MAX_RECORDS):
    if index >= len(records):
        break
    total += 1
    line = records[index]
    if len(line) > REC_CAP:
        counts[3] += 1
        continue
    tab = line.find(b"\t")
    number = None if tab < 0 else parse_number(line[:tab])
    if number is None:
        label = 3
    else:
        title = line[tab + 1:][:TITLE_MAX]
        label = 2
        if len(sec) < MAX_RECORDS:
            sec.append((number, title))
    counts[label] += 1
dig = b"total %d bug %d feature %d security %d other %d\n" % (total, *counts)
for k, (num, title) in enumerate(sec):
    line_len = 1 + len(str(num)) + 1 + len(title) + 1
    last = k + 1 == len(sec)
    if len(dig) + line_len + len(TRUNC_LINE) <= DIGEST_MAX or (last and len(dig) + line_len <= DIGEST_MAX):
        dig += b"#%d " % num + title + b"\n"
    else:
        dig += TRUNC_LINE
        break
print(json.dumps({"effect": dig.decode("latin-1"), "result": "ok %d" % total}))
PY
}
for name in fixture edge exact-last over-last exact-not-last over-not-last; do
  py "$here/generate/fake_hyalos.py" "$here/sdk/draft.py" "$here/scripts/triage.py" "$work/$name.tsv" 2 > "$work/$name.json"
  digest_port "$work/$name.tsv" > "$work/$name.port.json"
  py - "$work/$name.json" "$work/$name.port.json" "$name" <<'PY' || fail "triage.py differs from the digest guest on $name"
import json, sys
run, port, name = json.load(open(sys.argv[1])), json.load(open(sys.argv[2])), sys.argv[3]
assert run["effects"] == [[0, port["effect"]]], (run["effects"][:1], port["effect"][:200])
assert run["result"] == port["result"], (run["result"], port["result"])
body = port["effect"]
print(f"2.2: {name}: same as the digest guest ({len(body)} bytes,"
      f" {body.count(chr(10)) - 1 - body.endswith('...truncated' + chr(10))} security lines,"
      f" {'truncated' if body.endswith('...truncated' + chr(10)) else 'not truncated'}, {run['result']})")
PY
done

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
