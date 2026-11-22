# Context — what exists today

Every citation below was checked on 2026-11-22 at commit `72136c1`. Preflight (README rule 2) means
re-checking the ones your slice touches.

## 1. Repository shape

- **The work is in a spike.** `spikes/task-kernel` is not part of the root workspace, whose members are `core/*` and `crates/*` (`Cargo.toml:3`). CI does not build it, so this pack's tests run only when an executor runs them (§4).
- **Two Cargo builds matter here.** The monitor is the workspace at `spikes/task-kernel/Cargo.toml:3`, with the one member `kernel`. The attestation tool and notary are the separate crate `attest`, which declares its own workspace (`spikes/task-kernel/attest/Cargo.toml:7`).
- **Three programs take part in a run.**
  - The monitor (`task-kernel`) runs the guest, writes the public chain and audits a record (`spikes/task-kernel/kernel/src/main.rs:771`).
  - `attest` is the operator's tool, with `ask`, `verify` and `link-key` (`spikes/task-kernel/attest/src/main.rs:318`). The monitor starts it once per model call (`spikes/task-kernel/kernel/src/live.rs:173`).
  - `notary` is the customer's program, on the customer's machine (`spikes/task-kernel/attest/src/bin/notary.rs:1`).
- **Real evidence is committed.** `spikes/task-kernel/ledger/` and `spikes/task-kernel/out/` hold real version 1 chains and records, such as `spikes/task-kernel/out/live3.json`, which this work must not break.

## 2. What this work changes

### The record and its identity

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `spikes/task-kernel/kernel/src/main.rs` | 65–76 | `Answer`, with unknown fields refused and an optional `attestation` left out when empty | Gains the provider-proof slot in the proof slice |
| `spikes/task-kernel/kernel/src/main.rs` | 86–96 | `Transcript`, with a `version` field and unknown fields refused | Kept; version 2 is added by the format slice |
| `spikes/task-kernel/kernel/src/main.rs` | 343–359 | `execute` always writes `version: 1` | Changed by the format slice. Replay compares whole records (lines 361–371), so a replay must rebuild the version it was given |
| `spikes/task-kernel/kernel/src/main.rs` | 613–652 | `declared_run` writes the intent link and result link, each with `"v": 1` | Changed by the format slice |
| `spikes/task-kernel/kernel/src/main.rs` | 714–723 | `audit_record` parses a record and never reads its `version` | Changed: an unknown version is refused |
| `spikes/task-kernel/kernel/src/main.rs` | 726–749 | `audit_report`: what was proven, then five limits | Changed by the audit slice |
| `spikes/task-kernel/kernel/src/main.rs` | 760–763 | `task_of` hashes the guest and the inputs joined with no framing | Kept for version 1 only. Version 2 uses a framed form |
| `spikes/task-kernel/kernel/src/main.rs` | 766–769 | `claim_of` recomputes the record's task for the audit | Changed to choose the task form by the record's version |
| `spikes/task-kernel/kernel/src/ledger.rs` | 212–216 | `Claim`: the commitment, task and guest the audit holds the chain to | Kept |
| `spikes/task-kernel/kernel/src/ledger.rs` | 361–428 | `Auditor::audit`, which never reads a link's `v` | Changed: a link's version must match the record's |
| `spikes/task-kernel/kernel/src/ledger.rs` | 195–208 | `Audited`, with `runs` for this task and `all_runs` for any task under the key | Kept. `all_runs` already counts a run relabelled as another task, within one key |

### The notary and the tool

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `spikes/task-kernel/attest/src/bin/notary.rs` | 75–138 | `Sequences`: per-run counts in one file, saved before a number is used | Changed: each run also keeps its ticket |
| `spikes/task-kernel/attest/src/bin/notary.rs` | 283–310 | A session reads `release run`, spends the run's number and answers `ok` | Changed: the first line also names an authorisation |
| `spikes/task-kernel/attest/src/bin/notary.rs` | 399–417 | `sign` adds one extension, the run and its sequence number | Changed: adds the ticket extension |
| `spikes/task-kernel/attest/src/bin/notary.rs` | 445–468 | The serving command's arguments | Kept |
| `spikes/task-kernel/attest/src/bin/notary.rs` | 637–645 | `main` dispatches `report` or serves | Changed: adds `authorize` |
| `spikes/task-kernel/attest/src/lib.rs` | 35–37 | The `hyalos.run-sequence/1` extension's name and form | Kept |
| `spikes/task-kernel/attest/src/lib.rs` | 200–215 | Writes and strictly reads the run-sequence extension | Kept; the pattern for the ticket extension |
| `spikes/task-kernel/attest/src/lib.rs` | 457–465 | `link::open`: the operator's first message and the notary's one-word reply | Changed |
| `spikes/task-kernel/attest/src/main.rs` | 55–77 | `Word`: the closed set of refusals `ask` prints | Changed: adds the budget's refusals |
| `spikes/task-kernel/attest/src/main.rs` | 253–301 | `verify`: reads the presentation and reports what it attests | Changed: reports the ticket |
| `spikes/task-kernel/attest/src/main.rs` | 319–369 | `ask`, with the intent and position as arguments | Changed: takes the authorisation |
| `spikes/task-kernel/kernel/src/live.rs` | 27–47 | The notary settings a monitor passes to the tool | Changed: adds the authorisation |
| `spikes/task-kernel/kernel/src/live.rs` | 77–86 | Maps the tool's one word to a `Reason` | Changed |
| `spikes/task-kernel/kernel/src/main.rs` | 535–568 | `check_answer`: one answer against its attestation | Changed: checks the ticket |

## 3. Shared mechanics to reuse

- **Durable state.** Write a new file, sync it, rename it over the old one, then sync the directory: `Sequences::save` (`spikes/task-kernel/attest/src/bin/notary.rs:111`).
- **Refuse a state file you would not have written.** A header line, a counted last line and strict number forms: `Sequences::load` (`spikes/task-kernel/attest/src/bin/notary.rs:83`).
- **Spend before answering.** The number is saved before the operator can see any reply (ADR-014 of the notary pack): `spikes/task-kernel/attest/src/bin/notary.rs:304`.
- **One notary per directory.** An exclusive lock held until exit: `spikes/task-kernel/attest/src/bin/notary.rs:576`.
- **Private files.** `link::read_private` (`spikes/task-kernel/attest/src/lib.rs:245`) and `link::keep_private`, which writes whole or not at all and never over an existing file (`spikes/task-kernel/attest/src/lib.rs:261`).
- **An extension in the notary's signature.** Added with `Extension { id, value }` at `spikes/task-kernel/attest/src/bin/notary.rs:414`. The pinned TLSNotary builder appends each call to a list (`crates/attestation/src/builder.rs:110` in the TLSNotary repository, tag `v0.1.0-beta.1`, commit `40a0695`, pinned at `spikes/task-kernel/attest/Cargo.toml:14`), so an attestation should carry more than one. Slice 4 proves it in a session.
- **Read an extension strictly.** Exactly one with the name, in its one form: `read_run_sequence` (`spikes/task-kernel/attest/src/lib.rs:205`).
- **Hash text.** `binding` checks a 64-character lowercase hex value (`spikes/task-kernel/attest/src/lib.rs:73`). `sha` hashes bytes in the monitor (imported at `spikes/task-kernel/kernel/src/ledger.rs:10`).
- **Refusals are closed words.** The tool prints one `Word` (`spikes/task-kernel/attest/src/main.rs:57`), and the monitor maps it to a `Reason` (`spikes/task-kernel/kernel/src/live.rs:77`). Anything else is a refusal.
- **A failed call is never retried.** A retry would take the next number (`spikes/task-kernel/kernel/src/live.rs:162`).
- **The report's limits.** The hidden-bytes limit is a constant used by both reports (`spikes/task-kernel/kernel/src/main.rs:507`).
- **Test rigs.** `Rig` builds chains under test keys on a stand-in log (`spikes/task-kernel/kernel/src/audit_tests.rs:12`). `World` adds a real test notary and the stand-in provider (`spikes/task-kernel/kernel/src/forgery_tests.rs:24`). Attestation tests are in `spikes/task-kernel/attest/tests/`.
- **Repository rules that bind this work.** Fail closed and echo nothing (`CLAUDE.md:25`, `CLAUDE.md:26`). An agent never reads, prints or asks for a secret (`CLAUDE.md:19`).

## 4. How verification works here

CI does not build the spike. Each criterion is proven by a test the slice adds beside the code it changes, named with the criterion's ID, or by a command whose output is recorded in the progress file.

Run from `spikes/task-kernel`.

Build the monitor and the attestation tool:

```bash
cargo build --release
cargo build --release --manifest-path attest/Cargo.toml
```

Expected: each ends with a line starting `Finished`. The monitor prints one known warning, `direct cast of function item into an integer`, from `kernel/src/kvm.rs`.

Run the tests. The attestation tests need the built programs, so run them with a plain `cargo test` and no filter:

```bash
cargo test --manifest-path attest/Cargo.toml
cargo test
```

Expected: both end with `test result: ok`, with no test failed.

Check that older records still replay:

```bash
target/release/task-kernel replay guest/digest.wasm out/live3.json
```

Expected: `replay identical`.

Check that this work did not disturb the kernel's wall. This needs `/dev/kvm` and both kernel builds, made from `spikes/task-kernel/hyalos` with `cargo build --release` and `cargo build --release --features selftest --target-dir target-selftest`:

```bash
HYALOS_KERNEL=hyalos/target/x86_64-unknown-none/release/hyalos-kernel \
HYALOS_SELFTEST_KERNEL=hyalos/target-selftest/x86_64-unknown-none/release/hyalos-kernel \
target/release/task-kernel attacks guest/digest.wasm fixtures/issues.tsv
```

Expected: 51 lines starting `PASS`, none starting `FAIL`, exit status 0.

**What needs the operator.** A run against the real model needs the credential file, which only the operator's own account can read. An executor never opens it. A run that sends a notification posts a public comment and needs the operator's go-ahead each time. Every test in this pack uses the stand-in provider and a stand-in log.

## 5. Glossary

| Term | Meaning |
|---|---|
| Operator | Whoever runs the task. In this pack's threat model, the attacker |
| Customer | The party that runs the notary and decides how many attempts a job gets |
| Notary | The customer's program, which takes part in each provider session and signs an attestation |
| Task | A guest and its inputs, named by the task identity |
| Task identity | The hash naming a task. Version 2 frames each input's length |
| Run | One declared execution of a task, named by the hash of its intent link |
| Intent link, result link | The two public-chain entries around a run |
| Record | A run's transcript, its salt and its attestations |
| Record version | The `version` in a transcript, which sets how the record and its links are read |
| Authorisation | The customer's entry on the notary naming a task and how many attempts it gets |
| Attempt budget | The number of attempts in an authorisation |
| Ticket | One numbered attempt under an authorisation, spent by the first session of a run |
| Round | How many authorisations this notary had already made for the same task |
| Prior total | The sum of the budgets of a task's earlier authorisations on this notary |
| Ticket extension | The notary-signed part of an attestation holding the authorisation, task, budget, round, prior total and ticket |
| Provider proof | A proof from the model provider about how an answer was produced. None exists in practice today |
| Provider-proof slot | The typed place in a version 2 answer where such a proof would be named |
| Rehearsal | Asking the provider a question outside the notary, to see the answer before an attested run |
