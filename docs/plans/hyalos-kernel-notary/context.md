# Context — what exists today

Every citation below was checked on 2026-11-21 at commit `3ef845b`. Preflight (README rule 2) means
re-checking the ones your slice touches.

The spike landed on `main` in commit `1ccbe47`, with `spikes/task-kernel` identical to the tree at `3ef845b`, so every citation holds there unchanged.

## 1. Repository shape

- **The spike is outside the workspace.** `spikes/` is not part of the Cargo workspace and is not trusted core (`CLAUDE.md:3`). CI does not build or test it. A pull request that changes only `spikes/` and `docs/` skips the test jobs (`CLAUDE.md:42`).
- **Three separate Cargo builds live in `spikes/task-kernel`.**
  - The monitor, `task-kernel`, is the one member of the spike's own workspace (`spikes/task-kernel/Cargo.toml:1`). It runs a task as a process or boots the kernel on KVM, and holds the ledger and the audit.
  - The attestation tool, `attest`, is its own workspace (`spikes/task-kernel/attest/Cargo.toml:7`). It depends on TLSNotary from a git tag (`spikes/task-kernel/attest/Cargo.toml:10`).
  - The bare-metal kernel in `spikes/task-kernel/hyalos` and the guest in `spikes/task-kernel/guest` are not changed by this pack.
- **Build output is ignored.** `spikes/task-kernel/.gitignore` ignores `target/` and `target-selftest/`.
- **How the pieces meet.** The monitor's `live` command logs an intent link, runs the task, and logs a result link (`spikes/task-kernel/kernel/src/main.rs:544`, `:574`, `:581`). For each model call it starts `attest ask` as a child process when `HYALOS_ATTEST` is set (`spikes/task-kernel/kernel/src/main.rs:556`, `spikes/task-kernel/kernel/src/live.rs:59`). The audit then calls `attest verify` once per answer, only when `HYALOS_NOTARY` is set (`spikes/task-kernel/kernel/src/main.rs:508`, `:602`).

## 2. What this work changes

### The attestation tool

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `spikes/task-kernel/attest/src/main.rs` | 155–166 | Makes and reads the notary's signing key at `~/.config/hyalos-spike/notary.key`, in the same tool the operator runs | changed: the operator's tool stops reading any notary key |
| `spikes/task-kernel/attest/src/main.rs` | 175–222 | The notary's side of a session, as a function in the operator's process | changed: becomes a service of its own |
| `spikes/task-kernel/attest/src/main.rs` | 229–230 | Joins operator and notary with an in-memory pipe and starts the notary as a task in the same process | changed: a network connection to the customer's notary |
| `spikes/task-kernel/attest/src/main.rs` | 58–95 | Builds the one approved request: fixed model, fixed instructions, fixed answer schema, only the title varies; the credential is the last header's value | changed: the request also carries the run's binding |
| `spikes/task-kernel/attest/src/main.rs` | 262–268, 304–308 | Commits to the exchange in pieces, then reveals all of it except the credential's bytes and the reply's headers | kept |
| `spikes/task-kernel/attest/src/main.rs` | 316–355 | Verifies a presentation offline: trusted notary key, server name, the request is the approved template, the reply is a typed label | changed: also checks the binding and returns the session time for the audit |
| `spikes/task-kernel/attest/src/main.rs` | 358–387 | The commands `ask`, `verify` and `notary` (prints the local notary's public key) | changed: `notary` leaves this tool |

### The monitor

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `spikes/task-kernel/kernel/src/live.rs` | 49–71 | Starts `attest ask` for each question and takes its label, model, response ID and presentation hash; a failure to run the tool is `ModelUnavailable`, and output that is not a label is `ModelRefused` | changed: passes the run's binding and the notary's address |
| `spikes/task-kernel/kernel/src/main.rs` | 61–70 | An answer in the record: schema, question hash, label, model, response ID, and the presentation's hash when attested | kept |
| `spikes/task-kernel/kernel/src/main.rs` | 544–592 | `live`: intent link, run, result link, timestamp request, then writes the record and its salt | changed: the intent's hash reaches the attestation tool before the first model call |
| `spikes/task-kernel/kernel/src/main.rs` | 500–522 | `attested`: for each answer, the presentation's hash matches the record and `attest verify` reports the same question hash, label, model and response ID | changed: also the binding and the time window |
| `spikes/task-kernel/kernel/src/main.rs` | 595–606 | `audit`: recomputes the record's commitment, opens the ledger with the operator's home directory, and checks attestations only when `HYALOS_NOTARY` is set | changed: takes public keys only, and attestations are always checked |
| `spikes/task-kernel/kernel/src/ledger.rs` | 32–46 | Opens the chain and derives the public key from the operator's private key file with `openssl` | changed: the audit path never opens a private key |
| `spikes/task-kernel/kernel/src/ledger.rs` | 55–81 | Adds a link: signs it, posts it to the public log, keeps the log's reply beside the link | kept |
| `spikes/task-kernel/kernel/src/ledger.rs` | 85–91 | Requests a Bitcoin timestamp for a link and keeps the pending proof without verifying it | kept, unverified (prd Non-goals) |
| `spikes/task-kernel/kernel/src/ledger.rs` | 94–119 | Asks the public log which entries it holds under the operator's key and trusts the reply | changed: each entry's inclusion proof and signature are verified |
| `spikes/task-kernel/kernel/src/ledger.rs` | 122–159 | The chain rule: every link names the one before it, the log's entries under the key equal the chain, one result per intent | kept |

## 3. Shared mechanics to reuse

- **One approved request.** `body` builds it and `verify` rebuilds it from the title and demands identical bytes, so no other request can be attested: `spikes/task-kernel/attest/src/main.rs:58`, `:339`.
- **Where the credential sits.** `request` returns the byte range of the credential, which is the last header's value: `spikes/task-kernel/attest/src/main.rs:78`.
- **Hidden bytes are checked, not assumed.** `verify` requires exactly one hidden run in the request and that the header before it is the credential's: `spikes/task-kernel/attest/src/main.rs:329`.
- **The credential never reaches output.** `ask` reports a failure by its root cause with the credential's bytes replaced, and refuses to write a presentation that contains them: `spikes/task-kernel/attest/src/main.rs:368`.
- **The notary's session.** The verifier side of TLSNotary, with Mozilla's root certificates and a secp256k1 signing key: `spikes/task-kernel/attest/src/main.rs:175`, `:179`, `:202`.
- **Session size limits.** 8,192 bytes sent and 16,384 received, fixed before the session: `spikes/task-kernel/attest/src/main.rs:53`.
- **Reasons, not text.** The monitor maps every failure to a `Reason`: `spikes/task-kernel/kernel/src/main.rs:36`.
- **Hashing and hex.** `hex`, `unhex` and `sha`: `spikes/task-kernel/kernel/src/main.rs:167`, `:171`, `:178`.
- **Links are appended only through `Ledger::append`,** which refuses to keep a link the public log did not accept: `spikes/task-kernel/kernel/src/ledger.rs:55`.
- **The public log's address** is one constant: `spikes/task-kernel/kernel/src/ledger.rs:15`.
- **A forgery test already exists for the chain.** The `forge` command logs a second result for a run so the audit can be shown to refuse it: `spikes/task-kernel/kernel/src/main.rs:612`.
- **Repository rules that bind this work.** Fail closed and echo nothing (`CLAUDE.md:25`, `CLAUDE.md:26`). An agent never reads, prints or asks for a secret (`CLAUDE.md:19`).

## 4. Verify

The spike has no tests yet and CI does not build it. `cargo test` runs and finds none, so a Verify that reports zero tests has proven nothing. Each criterion is proven by a test the slice adds beside the code it changes, or by a command whose output is recorded against the criterion's ID in the slice's progress file.

Run from `spikes/task-kernel`.

Build the monitor and the attestation tool:

```bash
cargo build --release
cargo build --release --manifest-path attest/Cargo.toml
```

Expected: each ends with a line starting `Finished`. The monitor prints one known warning, `direct cast of function item into an integer`, from `kernel/src/kvm.rs`.

Build the bare-metal kernel twice, once plain and once with its self-test, from `spikes/task-kernel/hyalos`. Its build output is not in the repository:

```bash
cargo build --release
cargo build --release --features selftest --target-dir target-selftest
```

Expected: each ends with a line starting `Finished`.

Check that this work did not disturb the kernel's wall (needs `/dev/kvm` and both kernel builds), from `spikes/task-kernel`:

```bash
HYALOS_KERNEL=hyalos/target/x86_64-unknown-none/release/hyalos-kernel \
HYALOS_SELFTEST_KERNEL=hyalos/target-selftest/x86_64-unknown-none/release/hyalos-kernel \
target/release/task-kernel attacks guest/digest.wasm fixtures/issues.tsv
```

Expected: 51 lines starting `PASS`, none starting `FAIL`, exit status 0.

Check that older records still replay:

```bash
target/release/task-kernel replay guest/digest.wasm out/live3.json
```

Expected: `replay identical`.

**What needs the operator.** A run against the real model needs the credential file, which only the operator's own account can read. An executor never opens it. A run that sends a notification posts a public comment, so it needs the operator's go-ahead each time.

**Memory.** Run every command under a memory cap such as `ulimit -v 8000000`. An earlier bug in this spike exhausted the machine's memory.

## 5. Glossary

| Term | Meaning |
|---|---|
| Operator | Whoever runs tasks: holds the model credential and the ledger key, and runs the monitor |
| Customer | The party the operator's work is for. In this pack the customer runs the notary |
| Auditor | Anyone checking a record afterwards. Holds only public keys |
| Monitor | The program outside the kernel that answers its requests, calls the model and performs the effect (`task-kernel`) |
| Run | One execution of one task on fixed inputs |
| Record | The file a run leaves: inputs, answers, effects, result and fuel used. The code calls it a transcript |
| Ledger | The operator's chain of links, each also entered in the public log |
| Link | One entry in the ledger. An intent link is written before a run and a result link after it |
| Intent hash | The SHA-256 of a run's intent link. It names the run |
| Public log | The transparency log the ledger's links are entered in (Sigstore's Rekor) |
| Notary | The service that takes part in the TLS session without seeing its plaintext and signs what the server committed to |
| Notary key | The notary's signing key. Its public half is what an auditor trusts |
| Attestation | The notary's signed statement about one TLS session |
| Presentation | An attestation plus the parts of the exchange the operator chose to show. The file an auditor verifies |
| Binding | The run's intent hash and the question's position, carried inside the attested request |
| Session time | The notary's own clock, in whole seconds, read by the protocol library on the notary's side when the provider's server hello arrives. The library puts it in the attestation (`spikes/task-kernel/attest/src/main.rs:212`). It never comes from the operator's side: the author read this in the pinned library's source |
