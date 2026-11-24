#!/usr/bin/env python3
"""Lists every conditional jump in the elliptic-curve code of a built binary, with the four
instructions before it, so each can be read as public or secret. Used by RUSTSEC-2026-0387.md.

    objdump -d --no-show-raw-insn -C target/release/notary > notary.s
    python3 -I advisories/branches.py notary.s
"""
import re
import sys

PICK = re.compile(r"crypto_bigint|elliptic_curve::(scalar|secret_key)|k256::arithmetic|p256::arithmetic|primeorder|ecdsa::hazmat|rfc6979")
SKIP = re.compile(r"drop_glue|Debug|Display|serde|fmt::|Hash")
JCC = re.compile(r"\sj(?!mp\b)[a-z]+\s")

funcs, cur = {}, None
for line in open(sys.argv[1]):
    line = line.rstrip("\n")
    head = re.match(r"^[0-9a-f]+ <(.+)>:$", line)
    if head:
        cur = head.group(1)
        funcs[cur] = []
    elif cur and re.match(r"^\s+[0-9a-f]+:", line):
        funcs[cur].append(re.sub(r"^\s*[0-9a-f]+:\s*", "", line).split(" <")[0])

for name in sorted(funcs):
    if not PICK.search(name) or SKIP.search(name):
        continue
    body = funcs[name]
    jumps = [i for i, insn in enumerate(body) if JCC.search(" " + insn + " ")]
    print(f"{len(body):6} insns {len(jumps):3} jcc  {name}")
    for i in jumps:
        print("        " + " | ".join(insn[:34] for insn in body[max(0, i - 4) : i + 1]))
