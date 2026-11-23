# Context — what exists today

Every citation below was checked on 2026-11-22 at commit `fc063a2`. Preflight (README rule 2) means re-checking the ones your slice touches.

## 1. Repository shape

The work is in the Rust spike `spikes/task-kernel`, which is outside the main workspace, so CI does not build it. Local Verify is the real test. It has two parts:

- **The monitor**, crate `kernel`, binary `task-kernel`. It runs a guest, writes the record and audits it.
- **`attest`**, its own Cargo workspace, version 0.1.0. Its binaries are `attest` (`ask`, `verify`, `link-key`), the customer's `notary`, the test-only `standin`, and the test-only `forging-notary` behind the `forge-ticket` feature.

Build `attest` before running the monitor's tests, because they start its binaries. The forgery tests also need `forging-notary` built.

## 2. What this work changes

| File | Where | What it does now | Fate |
|---|---|---|---|
| `attest/probes/hidden_header.py` | whole file | The operator's manual probe: control, forged shape, control. Exit 0 refused, 1 accepted, 2 inconclusive | Kept as the operator's manual tool. Its pinned-body test stays |
| `attest/src/bin/notary.rs` | `spikes/task-kernel/attest/src/bin/notary.rs:192` | Spends a run's number and ticket in one durable save, after checking the authorisation | Changed: the canary is checked first |
| `attest/src/bin/notary.rs` | `spikes/task-kernel/attest/src/bin/notary.rs:645` | Signs the run-sequence and ticket extensions | Changed: also signs the canary |
| `attest/src/bin/notary.rs` | `spikes/task-kernel/attest/src/bin/notary.rs:895` | Dispatches `report`, `authorize`, `tickets`, or serves | Unchanged: the canary is a separate program |
| `attest/src/lib.rs` | `spikes/task-kernel/attest/src/lib.rs:40`, `spikes/task-kernel/attest/src/lib.rs:245` | The ticket extension and its strict reader | Pattern for the canary extension |
| `attest/src/lib.rs` | `spikes/task-kernel/attest/src/lib.rs:506` | The operator's side of the first link message and its one-word replies | Changed: gains `canary` |
| `attest/src/main.rs` | `spikes/task-kernel/attest/src/main.rs:57`, `spikes/task-kernel/attest/src/main.rs:266` | Ask's failure words, and offline verify's JSON | Changed: a canary word and a `canary` field |
| `kernel/src/main.rs` | `spikes/task-kernel/kernel/src/main.rs:35` | New runs write version 2 | Changed to 3 |
| `kernel/src/main.rs` | `spikes/task-kernel/kernel/src/main.rs:416` | Accepts versions 1 and 2 | Changed: accepts 3 |
| `kernel/src/main.rs` | `spikes/task-kernel/kernel/src/main.rs:666` | Per-answer checks of notary, run, position and ticket | Changed: checks the canary |
| `kernel/src/main.rs` | `spikes/task-kernel/kernel/src/main.rs:861` | The report's proven lines and limits | Changed: states the canary |
| `kernel/src/ledger.rs` | `spikes/task-kernel/kernel/src/ledger.rs:387`, `spikes/task-kernel/kernel/src/ledger.rs:435` | Accepts link versions 1 and 2, and checks the operator on version 2 intents | Changed: accepts 3, with the same operator check |
| `kernel/src/live.rs` | `spikes/task-kernel/kernel/src/live.rs:77` | Maps ask's word to a `Reason` | Changed: gains the canary reason |
| `attest/src/bin/standin.rs` | `spikes/task-kernel/attest/src/bin/standin.rs:81` | Answers every request with one status | Changed: can answer a sequence of statuses |
| `attest/tests/common/mod.rs` | `spikes/task-kernel/attest/tests/common/mod.rs:135` | Makes a test authorisation in the notary's directory | Changed: also a passed canary |
| `attest/probes/two_machines.sh` | `spikes/task-kernel/attest/probes/two_machines.sh:36` | Operator-only real run across two machines | Changed: requires a customer canary pass |
| `RECORD.md` | whole file | Defines record versions 1 and 2 | Changed: defines version 3 and the canary |

## 3. Shared mechanics to reuse

- Read a private file, refusing a link, a pipe or open permissions: `link::read_private`, `spikes/task-kernel/attest/src/lib.rs:290`.
- The durable replace pattern (write, sync, rename, sync the directory): `Sequences::save`, `spikes/task-kernel/attest/src/bin/notary.rs:173`.
- Strict parsing of a line-based state file: `exact`, `spikes/task-kernel/attest/src/bin/notary.rs:88`, and `parse_sequences`, `spikes/task-kernel/attest/src/bin/notary.rs:100`.
- The pinned credential's shape: `pinned_credential`, `spikes/task-kernel/attest/src/lib.rs:175`, and `CREDENTIAL_LENGTH`, `spikes/task-kernel/attest/src/lib.rs:34`.
- The approved body and the request head: `body`, `spikes/task-kernel/attest/src/lib.rs:114`, and `request`, `spikes/task-kernel/attest/src/lib.rs:135`.
- Provider trust settings for tests: `Trust::read`, `spikes/task-kernel/attest/src/lib.rs:75`.
- The signed session time: `ConnectionInfo.time` is set from the notary's TLS transcript, `spikes/task-kernel/attest/src/bin/notary.rs:657`.
- The test-only forging hook: `forged_ticket` in `attest/src/bin/forging-notary.rs`, used at `spikes/task-kernel/attest/src/bin/notary.rs:663`.
- The fixed limit text: `HIDDEN_BYTES_LIMIT`, `spikes/task-kernel/kernel/src/main.rs:573`.
- The probe's body is pinned to the template by a unit test, `spikes/task-kernel/attest/src/lib.rs:612`.

## 4. How verification works here

Run from `spikes/task-kernel`, in this order:

```bash
cargo build --release --manifest-path attest/Cargo.toml
cargo build --release --manifest-path attest/Cargo.toml --features forge-ticket --bin forging-notary
cargo test --manifest-path attest/Cargo.toml
cargo build --release
cargo test
target/release/task-kernel replay guest/digest.wasm out/live3.json
```

Expected: each build prints `Finished`, every suite prints `test result: ok`, and the replay prints `replay identical`. `cargo fmt --check` already differs in this spike, so do not reformat. A real provider is never contacted by a test.

## 5. Glossary

| Term | Meaning |
|---|---|
| Forged shape | A request whose hidden bytes hold a credential, a blank line and a small valid body, followed by the shown blank line and the start of the approved body |
| Canary check | One customer-run sequence of three provider calls: control, forged shape, control |
| Pass | A canary check where both controls got 200 and the forged shape got 400 |
| Held | The canary state after a check that did not pass or could not conclude. A later pass clears it |
| Latched | The canary state after the provider answered the forged shape with 200. Only the customer's reset clears it |
| Fresh | A pass no more than 24 hours older than the session it covers, and not later than it |
| Canary extension | `hyalos.canary/1`, the notary's signed statement of when the last pass happened |
| Customer | The party that runs the notary and holds the credential the canary uses |
| Operator | The party whose run is being checked, and whom the canary guards against |
