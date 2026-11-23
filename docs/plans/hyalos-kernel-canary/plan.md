# Plan

Run every Verify from `spikes/task-kernel`. A test proves an example row by naming it, for example `canary_s_01_2_e1`. No test or executor contacts the real provider, and no executor runs `attest/probes/two_machines.sh` for real.

## Traceability

| Criterion | Slices |
|---|---|
| S-01.1, S-01.2, S-01.3, S-01.4 | 1 |
| S-02.1, S-02.2, S-02.3 | 2 |
| S-03.1, S-03.2, S-03.3 | 3 |
| S-04.1, S-04.2 | 4 |

Example rows (S-NN.k.eN) follow their criterion's slices.

---

## Slice 1 — The customer can check the provider

Issue: #427 · Depends on: none · Stories: S-01 · Map: Check the provider

### Outcome

A customer can run `canary check` with their own credential and see one verdict, and the notary's directory then holds a pass, a hold or a latch. The notary does not read it yet.

### Acceptance

- **S-01.1** Only 200, 400, 200 records a pass.
- **S-01.2** Every other result leaves no pass, and a 200 on the forged shape latches.
- **S-01.3** The credential is checked before any provider call and is never echoed or stored.
- **S-01.4** A latch stays until the customer's reset.
- **1.1** The forged request's bytes equal those the Python probe builds for the same credential, shown by a test named `canary_1_1_*` that reads the probe's literals.
- **1.2** Two concurrent checks cannot both run: the second is refused by the lock, shown by a test named `canary_1_2_*`.
- **1.3** A default build ignores the rename-failure test setting, shown by a test named `canary_1_3_*`.

### Contracts

`canary check`, `canary reset`, the state file and the lock, as in `design.md` §2. `standin` gains `--statuses <code,code,…>`, one status per request in connection order, then 500 after the list ends. It overrides `--status` when both are given. Its existing controls are unchanged. The forged request's head copies the probe's `head` function, because `request` builds a head and body for one approved body and cannot place `SMALLEST`, the blank line and `APPROVED` together. The status list's position is shared across connections. Criterion 1.2's test holds `<d>/canary.lock` itself with `flock`. Canary tests use a `test-` credential of the pinned length, not the shared operator fixture. S-01.1.e2 can make the pass's write fail by creating `<d>/canary.new` as a directory, and S-01.2.e6 can stop staging by creating `<d>/canary.latched.new` as a directory. S-01.2.e7 fails that one rename through the setting `HYALOS_TEST_CANARY_FAIL_RENAME`, where any non-empty value fails only the rename of a staged non-pass result, honoured only under a new default-off Cargo feature `canary-test-hooks`, and its test is compiled only with that feature. Non-pass results are staged before the first call, as in `design.md` §2.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if any path would print, log or store the credential or a reply body (design P3).
- Stop if a pass could be recorded without both controls at 200 and the forged shape at 400.
- Stop if a passing check could clear a latch, including a latch hidden by a damaged state file (design P4).
- Stop if a completed non-pass check could leave a pass the notary would serve (ADR-003). The one accepted exception is ADR-007, a directory that refuses both the rename and the removal, where the check exits 2 and says to stop the notary.
- Stop if the notary binary would need to contain any of this credential handling (ADR-002).

### Verify

```bash
cargo build --release --manifest-path attest/Cargo.toml
cargo build --release --manifest-path attest/Cargo.toml --features forge-ticket --bin forging-notary
cargo test --manifest-path attest/Cargo.toml
cargo test --manifest-path attest/Cargo.toml --features canary-test-hooks --test canary
cargo build --release --manifest-path attest/Cargo.toml
cargo build --release
cargo test
target/release/task-kernel replay guest/digest.wasm out/live3.json
comm -23 <(grep -oE '^\| S-01\.[0-9]+\.e[0-9]+' ../../docs/plans/hyalos-kernel-canary/prd.md | sed 's/^| S-/canary_s_/; s/\./_/g' | sort -u) <(grep -rhoE 'fn canary_s_[0-9]+_[0-9]+_e[0-9]+' attest/src attest/tests kernel/src | sed 's/^fn //' | sort -u)
grep -c 'read_private' attest/src/bin/canary.rs
```

Expected: every build prints `Finished`, every suite prints `test result: ok`, the replay prints `replay identical`, and the `comm` command prints nothing. The last `grep` prints at least 1.

### Pattern to follow

- `spikes/task-kernel/attest/probes/hidden_header.py`, the three-call sequence and its verdicts.
- `link::read_private` and `Sequences::save`, cited in `context.md` §3.
- `attest/tests/common/mod.rs`, for starting `standin`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Add the status-sequence control to `standin`.
2. Add the `canary` binary. Put the strict state reader in `attest/src/lib.rs` from the start, since Slice 2's notary shares it.
3. Build the three requests from the pinned head and the probe's `SMALLEST` and `APPROVED` literals, copied verbatim.
4. Add the lock, the staged results, the `canary.init` marker, the latch and `reset`.
5. Add `attest/tests/canary.rs`, naming each example row.

### Files

Create `attest/src/bin/canary.rs`, `attest/tests/canary.rs`. Modify `attest/Cargo.toml` for the test feature, `attest/src/bin/standin.rs`, and `attest/src/lib.rs` for the state reader.

---

## Slice 2 — The notary holds runs without a fresh pass

Issue: #428 · Depends on: Slice 1 · Stories: S-02 · Map: Hold the notary

### Outcome

The notary refuses every session without a fresh pass, before spending anything, and signs the pass time into each attestation. The operator's run ends with a clear canary reason.

### Acceptance

- **S-02.1** A session opens only on a fresh pass, checked before anything is spent.
- **S-02.2** Every attestation signs its pass.
- **S-02.3** The notary holds no credential.
- **2.1** `attest verify` reports the canary field, and refuses a doubled or wrong-length extension, shown by tests named `canary_2_1_*`.
- **2.2** The attest package version is raised to 0.2.0, so an older tool hears `release`, shown by a test named `canary_2_2_*`.

### Contracts

The link word `canary`, the `hyalos.canary/1` extension, the `verify` JSON field, `Reason::CanaryHeld` and the freshness rule, as in `design.md` §2. The pass read at open is the one signed, carried in the session from the open to `sign`. The notary judges freshness at both points. S-02.2.e2 relies on the existing refusal of requester extensions, shown for the ticket by `record_4_2_a_requester_extension_is_refused` in `attest/tests/authorise.rs`.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if a session refused at open would spend a number or a ticket (design P1). The one accepted exception is S-02.1.e8, a session refused at signing, whose number was spent when it opened.
- Stop if the notary would open a credential or link any credential code.
- Stop if an unreadable, malformed or future state could be served.
- Stop if the notary could sign an attestation the audit's freshness rule would refuse.

### Verify

```bash
cargo build --release --manifest-path attest/Cargo.toml
cargo build --release --manifest-path attest/Cargo.toml --features forge-ticket --bin forging-notary
cargo test --manifest-path attest/Cargo.toml
cargo build --release
cargo test
target/release/task-kernel replay guest/digest.wasm out/live3.json
comm -23 <(grep -oE '^\| S-02\.[0-9]+\.e[0-9]+' ../../docs/plans/hyalos-kernel-canary/prd.md | sed 's/^| S-/canary_s_/; s/\./_/g' | sort -u) <(grep -rhoE 'fn canary_s_[0-9]+_[0-9]+_e[0-9]+' attest/src attest/tests kernel/src | sed 's/^fn //' | sort -u)
grep -c 'pinned_credential\|anthropic.key' attest/src/bin/notary.rs
```

Expected: every build prints `Finished`, every suite prints `test result: ok`, the replay prints `replay identical`, and the `comm` command prints nothing. The last `grep` prints 0.

### Pattern to follow

- The ticket extension and strict reader: `TICKET` and `read_ticket`, cited in `context.md` §2.
- The refusal before spending in `Sequences::spend`, and the `spent` word's path through `link::open`, `Word` and `refusal`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Add the extension and its strict reader to `attest/src/lib.rs`, and the field to `verify`.
2. Add `CANARY_WINDOW_SECONDS` beside the state reader in `attest/src/lib.rs`, and judge freshness in the notary, before the spend.
3. Judge it again at signing against the signed session time, then sign the pass time beside the ticket.
4. Carry the `canary` word through `open`, `Word`, `refusal` and `Reason`.
5. Make the test helpers write a passing state, then add the new tests.

### Files

Modify `attest/Cargo.toml`, `attest/Cargo.lock`, `attest/src/lib.rs`, `attest/src/main.rs`, `attest/src/bin/notary.rs`, `attest/tests/common/mod.rs`, `attest/tests/authorise.rs`, `kernel/src/main.rs`, `kernel/src/live.rs`, `kernel/src/live_tests.rs`, and any monitor test rig that starts a real notary.

---

## Slice 3 — The audit requires and states a fresh pass

Issue: #429 · Depends on: Slice 2 · Stories: S-03 · Map: Audit the record

### Outcome

New runs write record version 3. The audit refuses one without a fresh signed pass, and its report says when the check passed and by whom. Older records keep their audits.

### Acceptance

- **S-03.1** A version 3 audit requires one fresh signed pass per answer.
- **S-03.2** A pass states when, by whom and what it does not prove.
- **S-03.3** Older records keep their audits and cannot carry a canary.
- **3.1** The monitor's freshness constant equals the notary's, shown by a kernel test that reads `attest/src/lib.rs` with `include_str!`, because the monitor does not depend on the attest crate.
- **3.2** The provider-proof rules in `read_transcript` apply to version 3 as to version 2, so its `matches!(t.version, 1 | 2)` gate includes 3.
- **3.3** A version 3 intent naming another operator key is refused, as a version 2 intent is. The check at `kernel/src/ledger.rs` that matches `v == 2` must cover version 3.

### Contracts

Record version 3 and the report lines, as in `design.md` §2. `forging-notary` gains a hook read from `HYALOS_FORGE_CANARY`, in the style of `HYALOS_FORGE_TICKET`, with the values `missing`, `doubled`, `malformed` (a 7-byte value, for S-03.1.e4) and `at=<unix seconds>`. Monitor rigs get their pass by writing a `passed` state file directly, and S-03.2.e1 rewrites it between sessions to get two pass times.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if a canary could be accepted on a version 1 or 2 record (design P2).
- Stop if the forging hook could compile into the real `notary` (design P5).
- Stop if the committed records `out/live3.json` or `out/two-machines-record/record.json` would stop auditing or replaying.
- Stop if the hidden-bytes limit would leave any report.
- Stop if raising the version would drop the operator-key check from new intents.

### Verify

```bash
cargo build --release --manifest-path attest/Cargo.toml
cargo build --release --manifest-path attest/Cargo.toml --features forge-ticket --bin forging-notary
cargo test --manifest-path attest/Cargo.toml
cargo build --release
cargo test
target/release/task-kernel replay guest/digest.wasm out/live3.json
comm -23 <(grep -oE '^\| S-03\.[0-9]+\.e[0-9]+' ../../docs/plans/hyalos-kernel-canary/prd.md | sed 's/^| S-/canary_s_/; s/\./_/g' | sort -u) <(grep -rhoE 'fn canary_s_[0-9]+_[0-9]+_e[0-9]+' attest/src attest/tests kernel/src | sed 's/^fn //' | sort -u)
target/release/task-kernel audit out/two-machines-record/record.json ledger --operator-public out/two-machines-slice5/operator.pub --notary-public out/two-machines-slice5/notary.pub --guest guest/digest.wasm --verify attest/target/release/attest | grep -E '^audit passed|no provider canary'
```

Expected: every build prints `Finished`, every suite prints `test result: ok`, the replay prints `replay identical`, and the `comm` command prints nothing. The last command prints `audit passed` and `not proven: this record has no provider canary`. It needs the public log, so if the network is unavailable, record that in the progress file rather than treating it as a pass.

### Pattern to follow

- The ticket checks in `witnessed`, and `audit_report`, cited in `context.md` §2.
- `forged_ticket` in `attest/src/bin/forging-notary.rs`.
- The `record_s_*` forgery tests in `kernel/src/forgery_tests.rs`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Raise `CURRENT_VERSION`, and accept version 3 in `read_transcript` and the ledger, including its operator-key check.
2. Check the canary per answer in `witnessed`.
3. Add the report lines, with a UTC date formatted without a new dependency and unit-tested on 1970-01-01, 2000-02-29 and 2026-11-22.
4. Add the forging hook and the tests.

### Files

Modify `kernel/src/main.rs`, `kernel/src/ledger.rs`, `kernel/src/forgery_tests.rs`, `kernel/src/audit_tests.rs`, `attest/src/bin/forging-notary.rs`, and `attest/src/bin/notary.rs` only at the existing feature-gated hook.

---

## Slice 4 — The format and the real run are written down

Issue: #430 · Depends on: Slice 3 · Stories: S-04 · Map: Write it down

### Outcome

`RECORD.md` defines the canary and version 3, and the operator's real two-machine run refuses to start without a customer pass.

### Acceptance

- **S-04.1** The format document covers the canary.
- **S-04.2** The real probe will not publish without a pass.
- **4.1** `canary status --dir <d>` exits 0 and prints `fresh` on a fresh pass, and otherwise exits 1 and prints `not fresh`. It reads no credential.

### Contracts

`canary status`, as in `design.md` §2. `two_machines.sh` adds `canary` to its check for built programs, copies it beside the notary and runs `canary status --dir $remote/state` on that machine before it starts the notary, so a failure precedes every public action.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if the script's test would contact a real host, the provider, the public log or an issue.
- Stop if the script would read, copy or name the customer's credential: the customer runs `canary check` themselves.

### Verify

```bash
cargo build --release --manifest-path attest/Cargo.toml
cargo build --release --manifest-path attest/Cargo.toml --features forge-ticket --bin forging-notary
cargo test --manifest-path attest/Cargo.toml
cargo build --release
cargo test
target/release/task-kernel replay guest/digest.wasm out/live3.json
comm -23 <(grep -oE '^\| S-04\.[0-9]+\.e[0-9]+' ../../docs/plans/hyalos-kernel-canary/prd.md | sed 's/^| S-/canary_s_/; s/\./_/g' | sort -u) <(grep -rhoE 'fn canary_s_[0-9]+_[0-9]+_e[0-9]+' attest/src attest/tests kernel/src | sed 's/^fn //' | sort -u)
grep -c 'hyalos.canary/1' RECORD.md
grep -c '24 hours' RECORD.md
grep -ci 'canary reset' RECORD.md
bash -n attest/probes/two_machines.sh
```

Expected: every build prints `Finished`, every suite prints `test result: ok`, the replay prints `replay identical`, and the `comm` command prints nothing. Each `grep` prints at least 1, and `bash -n` prints nothing. S-04.1's rows are also proven by `canary_s_04_1_e*` tests that read `RECORD.md`. S-04.2's rows run the script from a temporary working directory, which has no `out/two-machines`, holding stub executables at the script's `$PWD` paths for `attest`, `notary` and `task-kernel`, with stub `ssh` and `scp` first on `PATH` and a temporary `HOME`. The stub `ssh` answers `-G` with a local listener's address, and exits 0 or 1 for `canary status` as the test sets, and prints the three `label value` lines the script reads from the notary. A test host with no `/usr/bin/time` skips with a named reason. The real `canary status` is proven separately by tests named `canary_4_1_*`. S-04.2.e1 is shown by the stub `task-kernel` recording its `live` call, and S-04.2.e2 by that record being absent. Nothing real is contacted.

### Pattern to follow

- `spikes/task-kernel/RECORD.md`, its existing sections and style.
- `attest/probes/two_machines.sh`, its `authorize` step.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Add `canary status`.
2. Add the gate to the script, before the notary starts.
3. Add the stubbed script tests.
4. Update `RECORD.md`, including an example cron line for `canary check` with `<notary-dir>` and `<credential-file>` placeholders, a statement that the operator-run script's gate guards the run and proves no independence, and the limit of ADR-007.

### Files

Modify `RECORD.md`, `attest/probes/two_machines.sh`, `attest/src/bin/canary.rs`, `attest/tests/canary.rs`.
