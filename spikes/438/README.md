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
`triage.py` under the host's Python with `fake_hyalos.py` and expects the digest guest's
notification format. CI does not run it. Each slice's progress file records its output.

## Run a spike task

```bash
cargo run --release --locked --manifest-path hyalos-os/Cargo.toml -p task-kernel --features spike-contract -- \
  spike-run <guest.wasm> spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py \
  hyalos-os/evidence/fixtures/issues.tsv record.json [seed]
```

`spike-task` takes the same five files and prints the task identity without running. On KVM, set
`HYALOS_KERNEL` to the path `spikes/438/kernel.sh <profile>` prints. The profile is `hyalos.spike/1`
unless `HYALOS_SPIKE_PROFILE` names another spike profile.
