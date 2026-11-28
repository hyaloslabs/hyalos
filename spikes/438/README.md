# Spike #438: a signed Python task with a contract-generated SDK

The rig behind [`docs/plans/hyalos-kernel-python`](../../docs/plans/hyalos-kernel-python/README.md).
Can a hyalOS task be a short Python script, run by an interpreter compiled into the guest, on
hyalOS's four calls, with a small SDK generated from its contract? This is **not product code**:
it is not in either Cargo workspace and not trusted core. The monitor's side of the spike, the
contract check and the `spike-run` and `spike-task` commands, is in `hyalos-os/` behind the
`spike-contract` feature (ADR-0136).

## What is here

| Path | Role |
| --- | --- |
| `contracts/` | The draft contracts, in canonical bytes with no trailing newline: `draft` (schema `0` and effect `0`), `no-notify` (schema `0` only), `no-ask` (effect `0` only) |
| `generate/generate.py` | The SDK generator: a contract file in, one Python source file out. A pure function of the contract's bytes, with a fixed name table (schema `0` is `ask.classify`, effect `0` is `notify.team`); a contract in any other byte form is refused |
| `generate/check.sh` | Checks the generator and the triage script (below) |
| `generate/fake_hyalos.py` | A host-side stand-in for a runtime guest's native `_hyalos` module, used only by `check.sh` |
| `sdk/` | The SDK generated from each contract, committed. Do not edit: regenerate them |
| `scripts/triage.py` | The issue-triage task in Python, against the SDK. It keeps every rule of the digest guest (`hyalos-os/guest/src/lib.rs`) |
| `kernel.sh` | `kernel.sh <profile>` builds the kernel image a profile's KVM runs need and prints its path |
| `fixtures/hostile.tsv`, `fixtures/collide.py` | The hostile inputs, and the generator and check of their colliding titles (Slice 5) |
| `micropython/`, `monty/` | Each runtime's build (Slices 3 and 4) |
| `guests/` | The committed `.wasm` guests that records name, each with its SHA-256 below (Slices 3 and 4) |
| `evidence/` | Records and `expected.tsv`, which `python_committed_evidence_replays_on_both_backends` replays (Slices 3 to 5); `evidence/data/` holds each hostile case's data file, as `hostile.sh` decodes it |
| `MEASUREMENTS.md`, `hostile.sh`, `measure.sh` | Measurements and the scripts that take them (Slice 5) |

Each upstream source a later slice builds is fetched by a script from its repository at a full
commit SHA the script checks after the fetch. A failed fetch or a different SHA stops the script.

## The SDK

The generated module `hyalos` has one public name, `task`. For the draft contract its public
members are `read_inputs`, `ask.classify`, `notify.team` and `finish`. `ask` exists only when the
contract grants a schema, and `notify` only when it grants an effect, so a script that uses an
ungranted member fails with an `AttributeError`. The SDK holds no client, key, model setting, URL or
tool list, and imports only `_hyalos`, the runtime guest's native module, which has exactly `read`,
`infer`, `effect` and `finish`. The SDK only shapes what a script sees. The monitor refuses any call
the contract does not grant, whatever the SDK says.

Under a spike profile the contract, the SDK and the script are input records 0, 1 and 2, so the task
identity covers all three: another contract gives other SDK bytes and another task.

## Regenerate the SDKs

Needs Python 3 on the host (checked with 3.12.3); the generator uses the standard library only.

```bash
for c in draft no-notify no-ask; do
  python3 -I spikes/438/generate/generate.py spikes/438/contracts/$c > spikes/438/sdk/$c.py
done
spikes/438/generate/check.sh    # ends "check: ok"
```

`check.sh` generates every SDK twice and compares both with the committed file. It checks that a
reordered, whitespace-padded or newline-terminated contract is refused, and that each SDK's public
names are the ones above. It checks that `triage.py` imports only `task` from `hyalos`. It also runs
`triage.py` under the host's Python with `fake_hyalos.py`: with every answer other it expects the
digest guest's notification format, and with every answer security it expects the bytes a
line-by-line port of the digest guest gives, on the fixture and on inputs at the guest's limits. CI does not run it. Each slice's progress file records its output.

## Run a spike task

```bash
cargo run --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --features spike-contract -- \
  spike-run <guest.wasm> spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py \
  hyalos-os/evidence/fixtures/issues.tsv record.json [seed]
```

`spike-task` takes the same five files and prints the task identity without running. On KVM, set
`HYALOS_KERNEL` to the path `spikes/438/kernel.sh <profile>` prints. The profile is `hyalos.spike/1`
unless `HYALOS_SPIKE_PROFILE` names another spike profile.

## MicroPython

`micropython/port/` is a MicroPython port for `wasm32-unknown-unknown` with no Emscripten and no
libc. It has no floats, no filesystem, no time and no `sys`, and uses a fixed 2 MiB GC heap that
is never collected, so a run that fills it ends with `MemoryError` the same way every time. Its
native module `_hyalos` has `read`, `infer`, `effect` and `finish` over the four `hyalos` imports.
`run` executes record 1 as module `hyalos` and record 2 as `__main__`. An uncaught exception ends
the run with `finish` of `error ` and the exception's type name.

MicroPython's non-local return (its exceptions) needs `setjmp` and `longjmp`, which plain
WebAssembly cannot express. LLVM lowers them only to the exception-handling proposal, which no
profile loads. The port gets them from Binaryen's Asyncify pass instead (`port/sjlj.c`): setjmp and
longjmp unwind the stack to `run`, which rewinds a saved snapshot. The output is plain WebAssembly
with no added import.

- Upstream: <https://github.com/micropython/micropython>, tag `v1.29.0`, commit
  `0fd6c573ea815774668bbb16b8e197c8822368b2`. `build.sh` fetches the tag and stops unless it is
  that commit. MicroPython's licence is `micropython/LICENSE`.
- Toolchain: `micropython/Containerfile`, on `ubuntu@sha256:562111b88caa79c71391c0c4d3c2114a7d5c35915c20561fd893cb1e07929e6b`, with
  clang 18.1.3 (`1:18.1.3-1ubuntu1`), wasm-ld 18.1.3 and Binaryen 108. The host needs only
  docker.
- `guests/micropython.wasm`: 249,874 bytes, SHA-256
  `590822a76c8ccbb2bd74d920bea3134ba671ad96c594f8888657fbd3b7c73b40`.

micropython profile: hyalos.spike/1

MicroPython runs on both backends. The default kernel image builds `wasmi` with `portable-dispatch`, which ended the stack overflow of issue #503 that used to end its loops in `KernelFault` on KVM (Slice 1 of `docs/plans/hyalos-kernel-capsules`). The guest's own sizes fit: 249,874 bytes against `GUEST_MAX` 1 MiB, and 4 MiB of memory.

```bash
spikes/438/micropython/build.sh   # two clean builds, compared; ends with the size and SHA-256
spikes/438/micropython/run.sh     # the micropython-* cases of evidence/expected.tsv; ends "micropython: ok"
```

`run.sh` runs each case on the backends `evidence/expected.tsv` lists, every MicroPython case on
both, and writes its record from the process leg; the KVM leg's record must be the same bytes. It
then runs the triage case once more on KVM and fails unless it gives its result there, so a return
of #503's `KernelFault` is noticed. On a host without KVM the first case's KVM leg fails the
script.

## Monty

`monty/guest/` is a crate that embeds [Monty](https://github.com/pydantic/monty), a Python
interpreter in Rust, built for `wasm32-unknown-unknown`. Its `run` compiles record 2 as `__main__`
and drives Monty one suspension at a time. `import hyalos` is answered with a module file whose
source defines `bytearray`, which Monty lacks and the generated SDK names, then executes record 1's
bytes unchanged as the module's body. `import _hyalos` is answered with a host module of `read`,
`infer`, `effect` and `finish` over the four `hyalos` imports. An uncaught exception ends the run
with `finish` of `error ` and the exception's type name. Time is fixed, `getrandom` is a constant
the guest supplies, and print goes nowhere. The guest imports the four `hyalos` functions and
nothing else.

Nothing in Monty's sources is edited. The build configuration that differs from Monty's own:
`chrono` without its default features (they read JavaScript's clock on this target), one line of
Monty's workspace `Cargo.toml` changed in the build's copy; `web-time` replaced through
`[patch.crates-io]` by `monty/guest/web-time`, a clock that never moves; and `getrandom`'s custom
backend.

- Upstream: <https://github.com/pydantic/monty>, tag `v1.6.0` (the tag the plan names; it
  exists), commit `3f89dcbf82071896efb5e3ded507db4191e8d829`. `build.sh` fetches the tag and stops
  unless it is that commit. Monty's licence is `monty/LICENSE`.
- Toolchain: rustc 1.99.1 (58cf8f9c3 2026-10-13), target `wasm32-unknown-unknown`, through
  rustup. `monty/guest/Cargo.lock` is Monty's own `Cargo.lock`, pruned by Cargo to what this
  target reaches; `build.sh` checks that every registry package in it is at Monty's version, and
  builds with `--locked`.
- `guests/monty.wasm`: 6,945,056 bytes, SHA-256
  `c301b1d6ca0c2c96d7c289a23ef003f1da375f9c97715c560086ff53f9c11baf`.

monty profile: hyalos.spike/2 — floats with `wasmi`'s deterministic results, `max_memory` 5 MiB, `fuel` 830,000,000. Peak memory 5 MiB: the smallest whole number of MiB at which the seed-0 triage run ends with a result (4 MiB ends `GuestTrap`), found under a local, uncommitted `hyalos.spike/2` with `max_memory` set to 4, 5, 6 and 7 MiB and `fuel` 20,000,000,000, running `HYALOS_SPIKE_PROFILE=hyalos.spike/2 hyalos-os/target/spike/release/task-kernel spike-run spikes/438/guests/monty.wasm spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py hyalos-os/evidence/fixtures/issues.tsv out.json 0`. Fixture fuel 413,707,242: the `fuel` that command's `ok:` line prints. The profile's fuel is twice that, rounded up to the next ten million.

process-only: monty — does not fit KVM: the guest is 6,945,056 bytes against `GUEST_MAX` 1 MiB (1,048,576 bytes), so the `spike-profile-2` image refuses it with `TooLarge` before boot. Its memory, 5 MiB, would fit the kernel's 16 MiB heap. No spike-only layout was built: one that held the guest would still leave the kernel stack overflow of issue #503, which ends any guest loop of about 10,000 iterations in `KernelFault`, and Monty's run is about 400 million fuel. That reason is inferred from #503, not shown on Monty.

typecheck: rejected — Monty's checker (`monty-type-checking` at the pinned commit, which embeds `ty`), given `monty/no-notify.pyi` as module `hyalos`, reports one error on `triage.py`: `unresolved-attribute`, `_Task` has no attribute `notify`, at line 50. With a stub that also types `notify.team`, it accepts the same script.

```bash
spikes/438/monty/build.sh       # two clean builds, compared; ends with the size, SHA-256 and rustc
spikes/438/monty/run.sh         # the monty-* cases of evidence/expected.tsv; ends "monty: ok"
spikes/438/monty/typecheck.sh   # Monty's type checker on triage.py against the no-notify stub
```

`run.sh` runs each case under `hyalos.spike/2` (`HYALOS_SPIKE_PROFILE`) on the backends
`evidence/expected.tsv` lists and writes its record. On a host with KVM it also checks that the
triage case still ends `TooLarge` on the `spike-profile-2` image (`kernel.sh hyalos.spike/2`,
`HYALOS_KERNEL`), so a machine that can hold the guest shows up as a failure and the cases can then
be listed `both`. Monty's records give the same effects, answers and results as MicroPython's, at
about 12.5 times the fuel.

## Hostile inputs and measurements

`fixtures/hostile.tsv` holds the hostile inputs (S-06.1): titles that are not UTF-8, a record of
exactly 16,384 bytes (the guests' read buffer) and one of 16,385, issue numbers the SDK must
refuse, titles aimed at the Monty guest's reading of bytes from their repr, 200 titles of 4,096
bytes aimed at MicroPython's uncollected heap and Monty's memory and fuel, 200 distinct titles as a
control, and a block of 200 colliding titles per runtime. Its first lines give its form, and a
comment names what each case tries.

The colliding titles come from `fixtures/collide.py`, which reimplements each runtime's hash of a
`bytes` value at the pinned commit, with its constant seed:

- MicroPython: `qstr_compute_hash`, `py/qstr.c:64-76`, djb2 with xor from 5381, masked to 16 bits
  (`py/qstr.c:48`, since the port's minimum ROM level leaves `MICROPY_QSTR_BYTES_IN_HASH` 0,
  `py/mpconfig.h:373-381`).
- Monty: `hash_python_bytes`, `crates/monty/src/hash.rs:187-191`, Rust's `DefaultHasher`
  (SipHash-1-3, both keys 0) over the slice's 4-byte length and its bytes. A dict or set is a
  hashbrown table probed by that hash's low bits (`crates/monty/src/types/hash_table.rs:114`).

Both were reproduced: a script that finishes with `hash()` of six titles, run in each committed
guest, gave the values `collide.py --hash` gives. So collisions were tested for both runtimes:
each block is 200 distinct 32-letter titles whose hashes agree in their low 16 bits, all of
MicroPython's hash and the bits that pick Monty's first probe in any table of up to 65,536 buckets.
`triage.py` never hashes a title, so neither block changes what the task does: its fuel is within
1,000 of the distinct control's under MicroPython, and both end `OutOfFuel` under Monty.

```bash
spikes/438/hostile.sh             # every case under each runtime that built; ends "hostile: ok"
spikes/438/measure.sh             # prints the tables of MEASUREMENTS.md
spikes/438/measure.sh --check     # stops unless MEASUREMENTS.md has the same figures but the times
```

`hostile.sh` decodes each case into `evidence/data/`, runs it at seed 0 on the backends its
runtime runs on (both for MicroPython, the process backend for Monty by its `process-only:` line), writes the record
of each run that ends with a result, and prints each run's outcome in `expected.tsv`'s form and
its reach: the labels asked, the notification's SHA-256, the result and the fuel. `expected.tsv`'s
hostile lines were copied from that output by hand. No hostile case has a `process-only:` line of
its own: each MicroPython case runs on KVM too, and gives the process leg's record there. What the cases could steer is in
`MEASUREMENTS.md`, and what it means for capsules in the pack's findings ADR
(`docs/plans/hyalos-kernel-python/decisions/ADR-008-findings-adopt-micropython-monty-is-not-ready.md`).
