# Context — what exists today

Every citation below was checked on 2026-11-22 at commit `c6ae5df`. Preflight (README rule 2) means
re-checking the ones your slice touches. Paths under `spikes/task-kernel/` move to `hyalos-os/` in
Slice 1, so after Slice 1 read them there.

## 1. Repository shape

- **The main workspace** is Rust, pinned to 1.99.1 (`rust-toolchain.toml:2`), with members `core/*` and `crates/*` (`Cargo.toml:3`). It forbids `unsafe` (`Cargo.toml:14`).
- **The trusted core** is capped at 30,000 lines (`crates/xtask/src/main.rs:17`). It stood at 28,594 on 2026-11-22. `cargo xtask layout` requires every workspace crate to live in `core/` or `crates/` (`crates/xtask/src/main.rs:274`), so hyalOS must be a separate workspace.
- **The spike** is at `spikes/task-kernel`, 302 tracked files, and is four separate Cargo builds:

| Directory | Package | Why it is built apart | Manifest |
|---|---|---|---|
| `kernel/` | `task-kernel`, the monitor and today's audit | Workspace member of `spikes/task-kernel` | `spikes/task-kernel/Cargo.toml:3` |
| `attest/` | `attest`, with the programs `attest`, `notary`, `canary`, `standin`, `forging-notary` | Its own `[workspace]`, because it pulls in TLSNotary | `spikes/task-kernel/attest/Cargo.toml:7` |
| `hyalos/` | `hyalos-kernel`, the bare-metal image | `no_std`, `panic = "abort"`, a linker script | `spikes/task-kernel/hyalos/Cargo.toml:7`, `spikes/task-kernel/hyalos/build.rs:3` |
| `guest/` | The example task | Built for WebAssembly, `no_std` | `spikes/task-kernel/guest/src/lib.rs:6` |

- **Decisions:** the parent's ADR-001 (own workspace at `hyalos-os/`), ADR-002 (keep `wasmi`, name the execution profile) and ADR-003 (v1 is the control plane) bind this pack. Under ADR-0129 (`docs/adr/0129-plan-packs-live-in-docs-plans.md`), a decision that changes something a `docs/adr/` ADR owns is made there as well.
- **The repository is public**, so GitHub build provenance is available to its releases (`.github/workflows/release.yml:9`).

## 2. What this work changes

### The audit

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `spikes/task-kernel/kernel/src/main.rs` | 26–37 | Limits: fuel, memory, sizes, labels, current version 3 | Limits move to the execution profile in Slice 2 |
| `spikes/task-kernel/kernel/src/main.rs` | 72–125 | The record's types, with unknown fields refused | Move to `hyalos-audit` in Slice 3 |
| `spikes/task-kernel/kernel/src/main.rs` | 150–197 | The host's read, infer and effect, with `Mode::Replay` | Replay path shared in Slice 3 |
| `spikes/task-kernel/kernel/src/main.rs` | 311–324 | The `wasmi` settings for the process backend | One definition in Slice 2 |
| `spikes/task-kernel/kernel/src/main.rs` | 326–373 | Runs a guest in process, with its import, export and store limits | Shared with the verifier in Slice 3 |
| `spikes/task-kernel/kernel/src/main.rs` | 381–415 | `execute_as` and `replay`, which compare the transcript byte for byte, `fuel_used` included | Version 4 added in Slice 2, shared in Slice 3 |
| `spikes/task-kernel/kernel/src/main.rs` | 419–440 | `read_transcript`: versions 1 to 3 | Version 4 in Slice 2, moved in Slice 3 |
| `spikes/task-kernel/kernel/src/main.rs` | 576 | `HIDDEN_BYTES_LIMIT`, a `not proven:` line of the audit report | Moves in Slice 3, wording preserved |
| `spikes/task-kernel/kernel/src/main.rs` | 604–729 | `check_answer` and `witnessed`, which run the external verify program | Move in Slice 3, in process in Slice 5 |
| `spikes/task-kernel/kernel/src/main.rs` | 796–820 | `audit_inputs`, including the required `--verify <program>` | Verifier's options in Slice 3, `--verify` gone in Slice 5 |
| `spikes/task-kernel/kernel/src/main.rs` | 831–837 | The stand-in log, chosen by environment variables | Test builds only, in Slice 3 |
| `spikes/task-kernel/kernel/src/main.rs` | 841–921 | `audit_record` and `audit_report`, with every "proven" and "not proven" line | Move in Slice 3, wording preserved |
| `spikes/task-kernel/kernel/src/main.rs` | 955–966 | `task_v2`, the framed task identity | Version 4 identity added in Slice 2 |
| `spikes/task-kernel/kernel/src/ledger.rs` | 35–38, 135–159 | Runs `openssl` for signatures through a temporary directory | Replaced for the audit in Slice 5 |
| `spikes/task-kernel/kernel/src/ledger.rs` | 166–190 | RFC 6962 inclusion proof | Moves in Slice 3 |
| `spikes/task-kernel/kernel/src/ledger.rs` | 230–459 | `Auditor`: entries, the log's list, chain rules, the claim | Moves in Slice 3 |
| `spikes/task-kernel/kernel/src/ledger.rs` | 117–134, 194–229 | `read_links`, public key reading, and the `Log`, `Audited` and `Claim` types | Move in Slice 3 |
| `spikes/task-kernel/kernel/src/ledger.rs` | 40–116 | Operator key generation, link signing and log submission | Stay in the monitor |
| `spikes/task-kernel/attest/src/main.rs` | 270–322 | `verify`: one presentation against the trusted notary | Moves to a library in Slice 5 |
| `spikes/task-kernel/attest/src/lib.rs` | 8–12 | Imports TLSNotary through the full `tlsn` crate | The check uses only `tlsn-attestation` and `tlsn-core` in Slice 5 |
| `spikes/task-kernel/hyalos/src/main.rs` | 169–181 | A second copy of the `wasmi` settings for the KVM backend | One definition in Slice 2 |
| `spikes/task-kernel/RECORD.md` | whole | The record format | Version 4 in Slice 2, verifier in Slice 7 |

### Repository wiring

| File | Lines | What it does now | Fate |
|---|---|---|---|
| `.github/workflows/ci.yml` | 48 | Treats `spikes/` as documentation, so spike tests never run in CI | `hyalos-os/` runs its own job from Slice 1 |
| `.github/workflows/ci.yml` | 59–60 (`every`, `touches`) | Decides which specialised jobs run from the paths changed | New `hyalos_os` output in Slice 1 |
| `.github/CODEOWNERS` | 10 | Names the trusted-core paths | `/hyalos-os/` added in Slice 1 |
| `CLAUDE.md` | 3, 42 | Describes spikes and what CI skips | Updated in Slice 1 |
| `deny.toml` | 52–54 | Denies git dependencies, for the main workspace | `hyalos-os/` gets its own policy in Slice 1, naming the TLSNotary source |
| `.github/workflows/release.yml` | 20 | The `hyalos` binary's release, on tags matching `v*`, which also matches `verifier-v*` | Slice 6 narrows the filter to `v[0-9]*`. The verifier gets its own workflow |
| `.github/scripts/release-evidence.sh` | 17 | Checks a fixed list of CI jobs for a commit | Slice 6 adds an optional job list, defaulting to today's |
| `.github/scripts/build-release-binary.sh` | 8, 15 | Builds in a digest-pinned image, with an unpinned `musl-tools` | The pattern for the verifier's build, which pins that package too |
| `crates/xtask/tests/release_gate.rs` | whole | Forbids workflows from creating tags or releasing without `--verify-tag` | Must keep passing, and must cover the new workflow in Slice 6 |

### Paths that break when the code moves

| File | Line | Assumes |
|---|---|---|
| `spikes/task-kernel/kernel/src/live_tests.rs` | 21 | `../attest/target/release` |
| `spikes/task-kernel/attest/tests/common/mod.rs` | 169 | `target/release/forging-notary` under `attest/` |
| `spikes/task-kernel/kernel/tests/audit_cli.rs` | 41 | `../../core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json`, two levels up |
| `spikes/task-kernel/attest/probes/two_machines.sh` | 7, 13–14 | Runs from `spikes/task-kernel`, with separate target directories |

## 3. Shared mechanics to reuse

- A refusal names its check and never a secret: `spikes/task-kernel/kernel/src/ledger.rs:370`.
- Everything is checked before anything is reported, so a failure states no proof: `spikes/task-kernel/kernel/src/main.rs:839`.
- The rules for version-specific records: `spikes/task-kernel/kernel/src/main.rs:419`.
- The framed, domain-tagged task hash to imitate for version 4: `spikes/task-kernel/kernel/src/main.rs:955`.
- Link versions the audit accepts: `spikes/task-kernel/kernel/src/ledger.rs:387`.
- The pinned public log key and URL: `spikes/task-kernel/kernel/src/ledger.rs:22` and `:25`.
- Reading only the bytes of a PEM public key, refusing a private key: `spikes/task-kernel/kernel/src/ledger.rs:122`.
- A test of the audit that uses an empty home directory and a stand-in log: `spikes/task-kernel/kernel/tests/audit_cli.rs:1`.
- The forgery corpus to carry over: `spikes/task-kernel/kernel/src/forgery_tests.rs` and `spikes/task-kernel/kernel/src/audit_tests.rs`.
- The test-only forging notary, gated by a feature: `spikes/task-kernel/attest/Cargo.toml` (`forge-ticket`).
- The release identifier, built from the TLSNotary tag and the crate's version: `spikes/task-kernel/attest/src/lib.rs:15`, `spikes/task-kernel/attest/build.rs:5`.
- Releasing a static binary with `SHA256SUMS` and provenance, from a tag a human pushed: `.github/workflows/release.yml:195` and `:207`.

## 4. How verification works here

The `attest` programs must be built before the monitor's tests, which run them. Without them, 83 of the monitor's 137 unit tests fail with `attest is not built` (`spikes/task-kernel/kernel/src/live_tests.rs:21`). The baseline at `c6ae5df`, run from `spikes/task-kernel`:

```bash
cargo build --release --locked --manifest-path attest/Cargo.toml --bins
cargo build --release --locked --manifest-path attest/Cargo.toml --features forge-ticket --bin forging-notary
cargo test --locked
cargo test --release --locked --manifest-path attest/Cargo.toml
```

Expected: **285 tests pass and none fail.** The monitor has 137 unit and 7 integration tests. `attest` has 12, 2, 3, 41, 10, 29, 4 and 40 across its binaries and test files. On the development machine the two `attest` builds took under a minute with a warm cache, and its tests about 160 seconds.

The main workspace's checks are in `CLAUDE.md`, **Before pushing**. A pull request that changes only `docs/` or `spikes/` skips the test jobs (`.github/workflows/ci.yml:48`).

## 5. Glossary

| Term | Meaning |
|---|---|
| Audit | Today's `task-kernel audit` command |
| Verifier | The packaged program this pack builds, `hyalos-verify` |
| Record | What a run leaves: its transcript, salt and presentations (`RECORD.md`) |
| Transcript | The record's JSON file |
| Record version | The transcript's `version` field and its links' `v` |
| Execution profile | The named, fixed set of interpreter version, settings and limits a replay runs under |
| Replay | Re-running the guest from the transcript and requiring identical bytes |
| Guest | The WebAssembly task |
| Monitor | The host program that runs a guest and performs its calls |
| Kernel image | The bare-metal program the KVM backend boots |
| Backend | Where a guest runs: in process, or on KVM |
| Link | One signed JSON entry in the operator's chain, an intent or a result |
| Chain | The operator's ordered list of links |
| Public log | Sigstore's Rekor transparency log |
| Presentation | The notary-signed proof of one model answer |
| Notary | The customer's service that signs attestations |
| Customer | The party who runs the notary and checks the record |
| Operator | The party who runs the task, and whom the check guards against |
| Limit | A "not proven" statement in the report |
| Genuine copy | A verifier binary the customer has confirmed came from this repository's tagged source |
| Inconclusive | The verifier could not finish a check, so it reports no pass |
