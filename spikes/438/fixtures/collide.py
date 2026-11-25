#!/usr/bin/env python3
"""Titles that collide in a runtime's string hash (Slice 5 of docs/plans/hyalos-kernel-python).

    collide.py <micropython|monty>            print the runtime's block of hostile.tsv
    collide.py --check <micropython|monty>    check that block in hostile.tsv
    collide.py --hash <micropython|monty> <hex>...   print each title's hash, in decimal

Each runtime hashes a `bytes` value with a fixed function and a constant seed, so a title's hash is
the same on every run and every host, and anyone who reads the source can aim at it:

- MicroPython v1.29.0 hashes a str or bytes with `qstr_compute_hash` (`py/qstr.c:64-76`): djb2
  with xor, starting at 5381, masked to 16 bits (`py/qstr.c:48`, the port's ROM level is the
  minimum, so `MICROPY_QSTR_BYTES_IN_HASH` is 0), and 0 is made 1.
- Monty v1.6.0 hashes bytes with `hash_python_bytes` (`crates/monty/src/hash.rs:187-191`): Rust's
  `DefaultHasher::new()`, SipHash-1-3 with both keys 0, over the slice's `Hash`: its length as a
  `usize` (4 bytes, little-endian, on wasm32) and then its bytes. A dict or set is a hashbrown
  table probed by that hash's low bits (`crates/monty/src/types/hash_table.rs:114`).

The block is 200 titles of 32 lowercase letters, all distinct, whose hashes agree in their low 16
bits: MicroPython's whole hash, and the bits that choose Monty's first probe in any table of up to
65,536 buckets. Each line is hostile.tsv's form: `collide`, the runtime, and the data line
`<number>\\x09<title>`, numbers 1 to 200. Generating Monty's block takes about a minute, so
`hostile.sh` runs `--check`, which hashes the committed block and needs no search.
"""

import sys

COUNT = 200
LENGTH = 32
MASK = 0xFFFF
LETTERS = b"abcdefghijklmnopqrstuvwxyz"
M64 = (1 << 64) - 1


def micropython(data):
    h = 5381
    for b in data:
        h = ((h << 5) + h) ^ b
    h &= MASK
    return h or 1


def _rotl(x, b):
    return ((x << b) | (x >> (64 - b))) & M64


def _round(v0, v1, v2, v3):
    v0 = (v0 + v1) & M64
    v1 = _rotl(v1, 13) ^ v0
    v0 = _rotl(v0, 32)
    v2 = (v2 + v3) & M64
    v3 = _rotl(v3, 16) ^ v2
    v0 = (v0 + v3) & M64
    v3 = _rotl(v3, 21) ^ v0
    v2 = (v2 + v1) & M64
    v1 = _rotl(v1, 17) ^ v2
    v2 = _rotl(v2, 32)
    return v0, v1, v2, v3


def _blocks(state, message):
    """SipHash-1-3's compression over the whole 8-byte blocks of `message`."""
    v0, v1, v2, v3 = state
    for i in range(0, len(message) - len(message) % 8, 8):
        m = int.from_bytes(message[i:i + 8], "little")
        v3 ^= m
        v0, v1, v2, v3 = _round(v0, v1, v2, v3)
        v0 ^= m
    return v0, v1, v2, v3


def _finish(state, tail, total):
    v0, v1, v2, v3 = state
    m = int.from_bytes(tail, "little") | ((total & 0xFF) << 56)
    v3 ^= m
    v0, v1, v2, v3 = _round(v0, v1, v2, v3)
    v0 ^= m
    v2 ^= 0xFF
    for _ in range(3):
        v0, v1, v2, v3 = _round(v0, v1, v2, v3)
    return v0 ^ v1 ^ v2 ^ v3


# SipHash's initial state for the keys (0, 0).
_START = (0x736F6D6570736575, 0x646F72616E646F6D, 0x6C7967656E657261, 0x7465646279746573)


def monty_raw(data):
    """`hash_python_bytes(data)`'s raw 64-bit value."""
    message = len(data).to_bytes(4, "little") + bytes(data)
    state = _blocks(_START, message)
    return _finish(state, message[len(message) - len(message) % 8:], len(message))


def monty(data):
    """`hash(data)` in Monty, as an int: the raw value as a signed 64-bit integer."""
    raw = monty_raw(data)
    if raw == M64:
        raw = M64 - 1
    return raw - (1 << 64) if raw >> 63 else raw


HASHES = {"micropython": micropython, "monty": monty}


def _prefix(index):
    """28 letters naming `index`, the same on every run."""
    out = bytearray(b"collide")
    n = index
    for _ in range(28 - len(out)):
        out.append(LETTERS[n % 26])
        n //= 26
    return bytes(out)


def _suffixes():
    for a in LETTERS:
        for b in LETTERS:
            for c in LETTERS:
                for d in LETTERS:
                    yield bytes((a, b, c, d))


def _search_micropython(prefix, target):
    for s in _suffixes():
        if micropython(prefix + s) & MASK == target:
            return prefix + s
    raise SystemExit("collide.py: no MicroPython title for this prefix")


def _search_monty(prefix, target):
    # The 4-byte length and the 28-byte prefix are four whole blocks, compressed once; each
    # candidate suffix is the last, partial block.
    head = (LENGTH).to_bytes(4, "little") + prefix
    state = _blocks(_START, head)
    total = len(head) + 4
    for s in _suffixes():
        if _finish(state, s, total) & MASK == target:
            return prefix + s
    raise SystemExit("collide.py: no Monty title for this prefix")


def block(runtime):
    search = {"micropython": _search_micropython, "monty": _search_monty}[runtime]
    first = _prefix(0) + b"aaaa"
    target = HASHES[runtime](first) & MASK
    titles = [first] + [search(_prefix(i), target) for i in range(1, COUNT)]
    return titles


def escape(data):
    out = []
    for b in data:
        if 0x21 <= b <= 0x7E and b != 0x5C:
            out.append(chr(b))
        else:
            out.append("\\x%02x" % b)
    return "".join(out)


def unescape(text):
    out = bytearray()
    i = 0
    while i < len(text):
        c = text[i]
        if c == "\\":
            if text[i + 1:i + 2] != "x" or len(text) < i + 4:
                raise ValueError("a backslash that is not \\xNN")
            out.append(int(text[i + 2:i + 4], 16))
            i += 4
        else:
            out.extend(c.encode())
            i += 1
    return bytes(out)


def lines(runtime, titles):
    return ["collide\t%s\t%d\\x09%s" % (runtime, n, escape(t)) for n, t in enumerate(titles, 1)]


def check(runtime, path):
    titles = []
    with open(path, encoding="ascii") as f:
        for line in f:
            if line.startswith("#"):
                continue
            fields = line.rstrip("\n").split("\t")
            if fields[:2] != ["collide", runtime]:
                continue
            number, title = unescape(fields[2]).split(b"\t", 1)
            if int(number) != len(titles) + 1:
                raise SystemExit("collide.py: %s's block is out of order" % runtime)
            titles.append(title)
    hashes = {HASHES[runtime](t) & MASK for t in titles}
    ok = (
        len(titles) == COUNT
        and len(set(titles)) == COUNT
        and {len(t) for t in titles} == {LENGTH}
        and len(hashes) == 1
    )
    if not ok:
        raise SystemExit("collide.py: %s's block is not %d distinct %d-byte titles of one hash"
                         % (runtime, COUNT, LENGTH))
    print("collide: %s %d titles, low 16 bits %04x" % (runtime, COUNT, hashes.pop()))


def main(argv):
    if len(argv) == 2 and argv[1] in HASHES:
        print("\n".join(lines(argv[1], block(argv[1]))))
    elif len(argv) == 4 and argv[1] == "--check" and argv[2] in HASHES:
        check(argv[2], argv[3])
    elif len(argv) >= 3 and argv[1] == "--hash" and argv[2] in HASHES:
        print(" ".join(str(HASHES[argv[2]](bytes.fromhex(h))) for h in argv[3:]))
    else:
        raise SystemExit(__doc__.split("\n\n")[1])


if __name__ == "__main__":
    main(sys.argv)
