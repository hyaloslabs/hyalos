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
| `fixtures/` | Hostile scripts and inputs (later slices) |
| `micropython/`, `monty/` | Each runtime's build (Slices 3 and 4) |
| `guests/` | The committed `.wasm` guests that records name, each with its SHA-256 below (Slices 3 and 4) |
| `evidence/` | Records and `expected.tsv`, which `python_committed_evidence_replays_on_both_backends` replays (Slices 3 to 5) |
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

process-only: micropython — does not fit KVM: the default kernel image runs out of its 2 MiB stack (`0x1e0_0000` to `0x200_0000`) and ends `KernelFault` once a guest loop runs about 10,000 iterations, because its `wasmi` dispatch does not get tail calls in the kernel build. MicroPython's start-up alone runs past that. The guest's own sizes fit: 249,874 bytes against `GUEST_MAX` 1 MiB, and 4 MiB of memory. This pack does not change the default image, so the fix is issue #503.

```bash
spikes/438/micropython/build.sh   # two clean builds, compared; ends with the size and SHA-256
spikes/438/micropython/run.sh     # the micropython-* cases of evidence/expected.tsv; ends "micropython: ok"
```

`run.sh` runs each case on the backends `evidence/expected.tsv` lists and writes its record. On a
host with KVM it also checks that the triage case still ends `KernelFault` there, so a fix for
#503 shows up as a failure and the cases can then be listed `both`.

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
- `guests/monty.wasm`: 6,944,896 bytes, SHA-256
  `31db15c5e91c455f775c550b7f2aa8eee962e58eaa8039c6986a102c98c823f3`.

monty profile: hyalos.spike/2 — floats with `wasmi`'s deterministic results, `max_memory` 5 MiB, `fuel` 830,000,000. Peak memory 5 MiB: the smallest whole number of MiB at which the seed-0 triage run ends with a result (4 MiB ends `GuestTrap`), found under a local, uncommitted `hyalos.spike/2` with `max_memory` set to 4, 5, 6 and 7 MiB and `fuel` 20,000,000,000, running `HYALOS_SPIKE_PROFILE=hyalos.spike/2 hyalos-os/target/spike/release/task-kernel spike-run spikes/438/guests/monty.wasm spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py hyalos-os/evidence/fixtures/issues.tsv out.json 0`. Fixture fuel 413,701,352: the `fuel` that command's `ok:` line prints. The profile's fuel is twice that, rounded up to the next ten million.

process-only: monty — does not fit KVM: the guest is 6,944,896 bytes against `GUEST_MAX` 1 MiB (1,048,576 bytes), so the `spike-profile-2` image refuses it with `TooLarge` before boot. Its memory, 5 MiB, would fit the kernel's 16 MiB heap. No spike-only layout was built: one that held the guest would still leave the kernel stack overflow of issue #503, which ends any guest loop of about 10,000 iterations in `KernelFault`, and Monty's run is about 400 million fuel. That reason is inferred from #503, not shown on Monty.

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
