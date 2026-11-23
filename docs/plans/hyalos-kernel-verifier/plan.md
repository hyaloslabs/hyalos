# Plan

The slices form one chain, each depending on the one before. Run every Verify command from the repository root.

## Dependency graph

```mermaid
flowchart LR
  S1[1 Move] --> S2[2 Profile] --> S3[3 Verifier] --> S4[4 Forgery corpus] --> S5[5 No external programs] --> S6[6 Genuine copy] --> S7[7 Real run and guide]
```

## Traceability

| Criterion | Slices |
|---|---|
| S-01.1, S-01.2, S-01.3, S-01.4, S-01.5 | 1 |
| S-02.1, S-02.2, S-02.3, S-02.4, S-02.5, S-02.6 | 2 |
| S-03.1, S-03.3, S-03.4, S-03.6, S-03.7 | 3 |
| S-03.2 | 4 |
| S-03.5, S-04.1, S-04.2, S-04.3, S-04.4 | 5 |
| S-05.1, S-05.2, S-05.3 | 6, 7 |
| S-05.4, S-05.5, S-05.6 | 6 |
| S-06.1, S-06.2, S-06.3, S-06.4 | 7 |

Example rows (S-NN.k.eN) follow their criterion's slices.

## Shared contract

These hold from the slice that introduces them to the end of the pack.

| Rule | Applies to | Requirement |
|---|---|---|
| Lint scope | `hyalos-profile`, `hyalos-audit`, `attest-check`, `hyalos-verify` | Each passes `cargo fmt --check` and `clippy -D warnings` from the slice that creates it |
| Moved code | `task-kernel`, `attest`, kernel image, guest | Not reformatted and not lint-fixed by this pack, except lines a slice must change anyway. The debt goes to `hyalos-kernel-core` |
| Features | `test-trust`, `forge-ticket` | Neither reaches a release build of `hyalos-verify` or `notary`. Release builds select the package with `-p`, never `--workspace`, because Cargo unifies features across selected packages. A `cargo test --release --workspace --features task-kernel/test-trust,hyalos-audit/test-trust` run is a test build and is exempt. Any binary that is published, or checked by S-03.7, is built with `-p hyalos-verify` and no features |
| Test names | Every criterion and local item | A test proving a PRD ID has it in its name, for example `s_03_6_e2_...`. A test proving a local item in Slices 3 to 7 is named `local_<slice>_<item>_...`, for example `local_5_2_...`, so it cannot be mistaken for a PRD ID |
| Tags | Every slice | No agent creates or pushes a `v*` or `verifier-v*` tag |

---

## Slice 1 — Move hyalOS to its own tested workspace

Issue: #443 · Depends on: none · Stories: S-01 · Map: Move and name

### Outcome

hyalOS lives in `hyalos-os/`, behaves exactly as it did, and its tests run in CI for the first time.

### Acceptance

- **S-01.1**
- **S-01.2**
- **S-01.3**
- **S-01.4**
- **S-01.5**
- **1.1** `spikes/task-kernel` no longer exists, and no tracked file outside `docs/plans/` refers to it as a current path.
- **1.2** Given a change only under `hyalos-os/`, when CI runs, then the `hyalos-os` job runs and `build, lint, test` requires its success. Given a change that does not touch it, then the job's skip does not fail the required check.
- **1.3** `docs/adr/` holds a new ADR for the `hyalos-os/` workspace, with its README row, because ADR-0017 owns the code layout.

### Contracts

Layout and directories: `design.md` §1. CI and repository rules: `design.md`, **Modifier: infra**. The monitor keeps its current commands and output in this slice.

| CI item | Requirement |
|---|---|
| `hyalos_os` output | True when `hyalos-os/`, `rust-toolchain.toml` or `ci.yml` changes. Unset on a push to `main`, and the job runs unless it is `false` |
| `hyalos-os` job | Builds `attest` first, runs every workspace test, and runs `cargo deny` with `hyalos-os/deny.toml` |
| `build, lint, test` | Needs this job, and accepts its `skipped` only when `hyalos_os` is `false` |
| Moved paths | The mapping in `design.md` §1. Tests find programs in `hyalos-os/target/release`. The fixture in `audit_cli.rs` is reached as `../core/hyalos/tests/fixtures/...` from the helper's `..` base |
| `hyalos-os/deny.toml` | Allows exactly the git source in `attest/Cargo.toml`, `github.com/tlsnotary/tlsn` |
| New ADR | The next free number in `docs/adr/` |

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if any evidence file's SHA-256 changes in the move (pre-mortem P1).
- Stop if the shared lockfile changes the resolved version of `tlsn`, `wasmi` or any cryptographic crate (R1).
- Stop if a test needs a behaviour change to pass after the move.
- Stop if `forge-ticket` reaches the `notary` program in a release build.

### Verify

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace
cargo deny --manifest-path hyalos-os/Cargo.toml check --config hyalos-os/deny.toml
cargo xtask layout
cargo xtask lines
git grep -n 'spikes/task-kernel' -- ':!docs/plans'
```

Expected: at least 285 tests pass and none fail. `deny`, `layout` and `lines` pass, and the line count equals the base commit's. The final `git grep` prints nothing.

Dry runs: compare a manifest of SHA-256 hashes taken at the base commit with the moved evidence (S-01.2). On the pull request, push one temporary failing test, see the required check go red, and remove it (S-01.3.e1).

### Pattern to follow

- Path-filtered CI output: `.github/workflows/ci.yml:59`.
- The required check's gathering: `.github/workflows/ci.yml:165`.
- Paths to fix: `context.md` §2, **Paths that break when the code moves**.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Hash all evidence and record resolved package versions at the base commit.
2. `git mv` the tree into the layout in `design.md` §1.
3. Add the workspace manifest and merge the lockfiles, keeping the version each lockfile resolves today, then compare.
4. Fix the paths. Keep the `tlsn = ` line in `attest/Cargo.toml`, because `attest/build.rs` reads it (R2).
5. Add CI, `deny.toml`, CODEOWNERS, the ADR and the `CLAUDE.md` wording.

### Files

- Create: `hyalos-os/Cargo.toml`, `hyalos-os/Cargo.lock`, `hyalos-os/deny.toml`, a new `docs/adr/` ADR
- Modify: `.github/workflows/ci.yml`, `.github/CODEOWNERS`, `CLAUDE.md`, `docs/README.md`, `docs/adr/README.md`, `.gitignore`
- Move: `spikes/task-kernel/**` to `hyalos-os/**`, by the mapping in `design.md` §1

---

## Slice 2 — Name the execution profile in every new record

Issue: #444 · Depends on: Slice 1 · Stories: S-02 · Map: Move and name

### Outcome

Both backends build from one profile, new runs write version 4 records that bind it, and every older record still replays.

### Acceptance

- **S-02.1**
- **S-02.2**
- **S-02.3**
- **S-02.4**
- **S-02.5**
- **S-02.6**
- **2.1** Given a guest and inputs, when `task-kernel task` runs, then it prints the version 4 identity (ADR-002).
- **2.2** The kernel image's `wasmi` version equals the workspace's, enforced by a monitor test that reads `../kernel-image/Cargo.lock`.
- **2.3** The `hyalos-os` CI job runs the backend comparison on the runner's KVM with a new `HYALOS_REQUIRE_KVM` set, and this slice adds that variable to the `HYALOS_REQUIRE_*` list in `CLAUDE.md`. Without it, a host with no KVM reports the comparison as skipped. CI uses `ubuntu-24.04`, whose KVM the microVM job already uses (`.github/workflows/ci.yml:143`).

### Contracts

`design.md` §2, **`hyalos-profile`** and **Record version 4**; ADR-002 and ADR-007. `RECORD.md` documents version 4 in this slice.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if any committed version 1, 2 or 3 record stops replaying or changes a byte (pre-mortem P2).
- Stop if the two backends give different records for one guest and inputs (R3).
- Stop if defining the profile would change `wasmi`'s version or any setting.
- Stop if KVM cannot be made available to the CI job, because S-02.5 would then be unproven.

### Verify

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
HYALOS_REQUIRE_KVM=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace
cargo fmt --manifest-path hyalos-os/profile/Cargo.toml --check
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-profile --all-targets -- -D warnings
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace s_02
```

Expected: all pass, and the last command runs at least one test per S-02 example row.

### Pattern to follow

- Framed identity: `hyalos-os/monitor/src/main.rs`, `task_v2` (`context.md` §3).
- Version-specific rules: `read_transcript` (`context.md` §3).
- Settings to unify: the two `wasmi` configurations in `context.md` §2.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Add `hyalos-profile` and move both configurations onto it, with no record change.
2. Add the version 4 transcript, identity and link version.
3. Update every version gate so version 4 carries each version 3 requirement: `CURRENT_VERSION`, the task identity arm in `execute_as`, `read_transcript`, `claim_of`, the report's version conditions, the link checks at `ledger.rs:387` and `:436`, and `main.rs:421` and `:433`. Search for others and list them in the progress file. The `profile` field is left out when serializing versions 1 to 3, so their bytes do not change. Each backend keeps its `wasmi` crate features, and S-02.5 proves they behave alike.
4. Add the KVM comparison and CI step, then update `RECORD.md`.

### Files

- Create: `hyalos-os/profile/`
- Modify: the monitor, kernel image, `hyalos-os/RECORD.md`, `.github/workflows/ci.yml`, `CLAUDE.md`, workspace manifest and lockfiles

---

## Slice 3 — Give the customer one verifier program

Issue: #445 · Depends on: Slice 2 · Stories: S-03 · Map: Check a record

### Outcome

`hyalos-verify` checks a record through a shared audit library, with clear pass, refusal and inconclusive results, and without the code that runs tasks.

### Acceptance

- **S-03.1**
- **S-03.3**
- **S-03.4**
- **S-03.6**
- **S-03.7**
- **3.1** The monitor's `audit` command calls `hyalos-audit`, and for inputs within the limits in `design.md` §2 its options, output and exit behaviour are unchanged, including its `ledger` default for the links directory, which `hyalos-verify` does not have. Local file limits are in `hyalos-verify`, so the monitor keeps its reads. The log limits Slice 5 adds are in `hyalos-audit`, so they apply to both. Slice 5 then removes its `--verify`.
- **3.2** `cargo tree -p hyalos-verify -e features` shows no `test-trust` in a release selection.
- **3.3** Every audit error is mapped to refused or inconclusive by a table test (R7).
- **3.4** Until Slice 5, `hyalos-verify` requires `--verify <program>`, as the audit does today (`design.md` §2). A program that cannot be launched exits 2.
- **3.5** All five stand-in variables in `design.md`, **Modifier: infra**, are gated in the code `hyalos-verify` links. A build without `test-trust` launches the child `attest verify` with the three provider variables removed from its environment, so S-03.7.e2 holds through the child too. A `test-trust` build passes them through, so the stand-in tests still work. The monitor's reads are gated by its own `test-trust` feature, which forwards to `hyalos-audit`. `attest`'s programs keep reading the provider variables.
- **3.6** Each golden report is in `hyalos-os/audit/tests/golden/`, named by the record's path under `hyalos-os/` with `/` replaced by `-`, so two records cannot collide. The progress file lists each record's inputs. The golden set is every committed record the audit passes, plus every transcript that `audit_cli.rs` or `audit_tests.rs` audits to a pass. Those tests make their keys at run time, so each is captured once at the commit this slice branches from, before any audit code moves, using a temporary uncommitted patch that copies each record before cleanup: its transcript, public keys, stand-in log and report are committed under `hyalos-os/audit/tests/golden/`, and no private key is. A committed record uses the keys in its own directory, or those in `out/two-machines-slice5/` for a version 1 record. A record that passes under no committed key set is not golden, and the progress file says so. A refusal has no report, so refused transcripts are not golden. The monitor's tests and Slice 4 hold their refusals.
- **3.7** After this slice, `hyalos-verify` reports a refusal with exit 1 on a record whose chain or answer is altered, proven by a test named `local_3_7_...`. This is a different item from S-03.7, whose test is `s_03_7_...`. Slice 4 carries over the full forgery corpus.

### Contracts

`design.md` §2: **`hyalos-verify` command**, **Size limits** and **`hyalos-audit`**; ADR-003 and ADR-004.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if a golden report would have to change to make a test pass.
- Stop if any of the monitor's existing forgery tests stops refusing after the extraction (pre-mortem P3).
- Stop if `test-trust` can reach a release build of `hyalos-verify`, or if a provider trust variable can reach its child `attest verify` (pre-mortem P3).
- Stop if the replay host cannot be shared without linking the live path into `hyalos-audit`.

### Verify

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust
cargo fmt --manifest-path hyalos-os/audit/Cargo.toml --check
cargo fmt --manifest-path hyalos-os/verify/Cargo.toml --check
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-audit -p hyalos-verify --all-targets -- -D warnings
cargo tree --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify -e normal --prefix none | grep -E '^(kvm-ioctls|kvm-bindings|tlsn|tokio-rustls|rcgen) '
cargo tree --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify -e features | grep 'test-trust'
```

Expected: all pass, and each of the final two `grep` commands prints nothing.

Dry runs: capture every golden report from the pre-extraction monitor, and commit them before moving any audit code (S-03.1). A test named `s_03_7_...` builds `-p hyalos-verify` in release without `test-trust`, in its own target directory, and runs it with all five stand-in variables set (S-03.7).

### Pattern to follow

- The audit to extract: `context.md` §2, **The audit**.
- A CLI test with an empty home and a stand-in log: `hyalos-os/monitor/tests/audit_cli.rs:1`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Capture and commit the golden reports.
2. Extract `hyalos-audit`, keeping the monitor green at each step.
3. Put the stand-in settings behind `test-trust`.
4. Add `hyalos-verify` with bounded reading and the verdict-to-exit mapping.

### Files

- Create: `hyalos-os/audit/`, `hyalos-os/verify/`, the golden report files
- Modify: the monitor, workspace manifest and lockfile, `.github/workflows/ci.yml`

---

## Slice 4 — Prove the verifier refuses every known forgery

Issue: #446 · Depends on: Slice 3 · Stories: S-03 · Map: Check a record

### Outcome

The built `hyalos-verify` refuses every forgery the monitor's tests know about, from committed fixtures, without linking the monitor.

### Acceptance

- **S-03.2**
- **4.1** The corpus for S-03.2 is every test in `forgery_tests.rs` and `audit_tests.rs` that runs the audit on a record and asserts a non-pass outcome. A test that refuses during a live run, before any record exists, is outside the corpus, and the manifest names it with that reason. The monitor keeps those tests. Integration tests in `hyalos-os/verify/tests/` run only the built `hyalos-verify` on the fixtures, with helpers in `tests/common/`, and report how many cases they ran. That number equals the number of cases in `hyalos-os/verify/tests/fixtures/manifest.json`, which lists each case, one audit call on a forged input, under the test it comes from. `local_4_2_generate` fails if the manifest's test names differ from the refusing tests. They run the workspace test build with `test-trust`, and find `attest` and `forging-notary` in `hyalos-os/target/release`, failing if either is missing. S-03.2 is proven on this `test-trust` build because the fixtures use a stand-in log, and S-03.7 separately proves that a release build ignores stand-ins.
- **4.2** A monitor test, `local_4_2_generate`, owns the fixtures in `hyalos-os/verify/tests/fixtures/`: each forged record, link set, stand-in log and an `expected.json` per case naming the verdict and the check that refuses it. It uses the process backend, so it needs no KVM. With `HYALOS_REGENERATE_FIXTURES=1` it rewrites them. Otherwise it regenerates each case in a temporary directory and fails unless the verdict and named check equal the expected file. Signatures differ on each run, so it never compares them byte for byte.
- **4.3** The generator uses throwaway keys, and only public keys are committed.
- **4.4** Given a record with an unknown provider proof, when the verifier checks it, then the report keeps its line `unchecked provider proofs: 1, which prove nothing` (`spikes/task-kernel/kernel/src/main.rs:904`). The line prints only for a ticketed record, so this fixture is made through the ticketed path `forgery_tests.rs` uses (`forgery_tests.rs:998`). This record and the one in 4.5 are passes, kept in `hyalos-os/verify/tests/fixtures/pass/`, outside the S-03.2 manifest.
- **4.5** The generator also writes one 5-answer record against the stand-in provider, which Slice 5 times for 5.6.
- **4.6** `hyalos-verify` has no normal or dev dependency on `task-kernel`.

### Contracts

None new. `design.md` §2, **`hyalos-verify` command**, and §6, **Test strategy**.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if a forgery in the corpus exits 0 (pre-mortem P3).
- Stop if a forgery test cannot be written as a fixture that the built verifier can check. Do not drop the case.
- Stop if the verifier would need to depend on the monitor to run a case.

### Verify

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust
cargo fmt --manifest-path hyalos-os/verify/Cargo.toml --check
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-audit -p hyalos-verify --all-targets -- -D warnings
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust -- s_03_2 local_4
cargo tree --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify -e normal,dev --prefix none | grep '^task-kernel '
```

Expected: all pass, the final `grep` prints nothing, and the S-03.2 test reports the same number of cases as the fixture manifest.

### Pattern to follow

- The corpus: `hyalos-os/monitor/src/forgery_tests.rs` and `hyalos-os/monitor/src/audit_tests.rs`.
- A CLI test with an empty home and a stand-in log: `hyalos-os/monitor/tests/audit_cli.rs:1`.
- How the `attest` tests launch the forging notary: `hyalos-os/attest/tests/common/mod.rs:169`. After Slice 1 it resolves from `hyalos-os/target/release`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. List every refusing test in a manifest.
2. Add the generator and its drift check in the monitor.
3. Commit the fixtures and their expected files.
4. Add the verifier's integration tests over them.

### Files

- Create: `hyalos-os/verify/tests/fixtures/`, `hyalos-os/verify/tests/common/`, the verifier's corpus tests, the monitor's generator test
- Modify: `hyalos-os/verify/Cargo.toml` (dev-dependencies only, if any)

---

## Slice 5 — Remove every external program from the check

Issue: #447 · Depends on: Slice 4 · Stories: S-03, S-04 · Map: Check a record, Stand alone

### Outcome

With an empty `PATH`, a test build of the verifier passes a real record against the stored stand-in log, using in-process checks shown to agree with `openssl` and `attest verify`. Slice 7 proves the same on the public log.

### Acceptance

- **S-03.5**
- **S-04.1**
- **S-04.2**
- **S-04.3**
- **S-04.4**
- **5.1** `attest-check` depends on `tlsn-attestation` and `tlsn-core`, not `tlsn`, with the same root store, from the `mozilla-certs` feature of `tlsn-core`. `attest verify` keeps its own full-`tlsn` check, unchanged, because it is the independent judge for S-04.2. Only the audit calls `attest-check`, and the comparison stays in CI to catch any drift between the two. Its default real trust, the server name (`attest/src/lib.rs:21`) and the Mozilla roots with address `server:443` (`Trust::read`, `attest/src/lib.rs:78`), is copied as plain values, because `attest-check` must not depend on `attest`.
- **5.2** Log replies over 1 MiB and lists of more than 10,000 entries under one key are inconclusive, and redirects are not followed. This slice adds that setting to the agent at `ledger.rs:248`, which today sets only a timeout and which the monitor's audit shares.
- **5.3** All three `openssl` audit sites in `ledger.rs` (log entry, signed tree note, link) use `p256` and are in the differential test, with one bit-flipped copy of each signature (S-04.3.e2).
- **5.4** The presentation comparison is an integration test in `hyalos-os/verify/tests/` that launches the built `attest verify`, and covers the full JSON it prints, key by key, with `attest-check` returning the same keys. Each presentation is judged under the real provider's default trust and the `notary.pub` of its record's directory, or, where none is committed (`out/live3.json.attest`, `out/two-machines/`, `out/q.presentation`), the key the presentation names, so that case compares only the check and not who is trusted. A presentation made against a stand-in provider is refused by both under this trust, and counts as refused. Presentations with no committed notary key are still judged by both and must agree, but are counted separately, outside the both-accept requirement. The test reports how many both accept and how many both refuse, and fails unless both accept all 40 in `out/two-machines-canary/record.json.attest`. The certificate check uses the connection time inside the attestation, not the wall clock (TLSNotary `v0.1.0-beta.1`, `crates/attestation/src/presentation.rs:82`), so the result does not change with the date and the test sets no clock. If `attest verify` refuses any of those 40, stop. One copy of each presentation with one byte changed must be refused by both (S-04.2.e2).
- **5.5** The monitor's `audit` no longer takes `--verify`, and its report is unchanged.
- **5.6** The progress file records the time to verify the generator's 5-answer record with a stored log on the `test-trust` build, and the non-blank, non-comment lines in `src/` of `hyalos-verify`, `hyalos-audit` and `attest-check`. The slice is not done if the time is 10 seconds or more, or the lines exceed 3,000.

### Contracts

`design.md` §1 (`attest-check`) and §2; ADR-005 as amended by ADR-008. Trust settings are passed by value (ADR-004).

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if `p256` and `openssl` disagree on any signature, valid or altered (pre-mortem P4, R4).
- Stop if the in-process presentation check and `attest verify` disagree on a verdict or field (R5).
- Stop if `attest-check` cannot be built without the full `tlsn` crate.

### Verify

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --bins
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p attest --features forge-ticket --bin forging-notary
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust
cargo fmt --manifest-path hyalos-os/attest-check/Cargo.toml --check
cargo clippy --locked --manifest-path hyalos-os/Cargo.toml -p attest-check -p hyalos-audit -p hyalos-verify --all-targets -- -D warnings
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust -- s_03_5 s_04 local_5
cargo tree --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify -e normal --prefix none | grep -E '^(kvm-ioctls|kvm-bindings|tlsn|tokio-rustls|rcgen) '
git grep -n 'Command::new' -- hyalos-os/audit hyalos-os/verify/src hyalos-os/attest-check
```

Expected: all pass, and the final `cargo tree` and `git grep` commands print nothing. The differential tests report how many items they compared, including all 171 committed presentations.

### Pattern to follow

- Today's presentation check: `attest/src/main.rs`, `verify` (`context.md` §2).
- Reading only a PEM public key: `public_der` (`context.md` §3).

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Create `attest-check` from a copy of the check, leave `attest verify` as it is, and add the presentation differential test.
2. Add the `p256` check and its differential test against `openssl`.
3. Switch the audit to both, and remove `--verify` from the monitor's audit and `hyalos-verify`, and remove the temporary directory. `attest` is not changed. `attest-check` holds its own trust type of DER roots.

### Files

- Create: `hyalos-os/attest-check/`
- Modify: `hyalos-os/audit/`, `hyalos-os/monitor` (its audit command), `hyalos-os/verify/`, workspace manifest and lockfile

---

## Slice 6 — Let a customer confirm a genuine copy

Issue: #448 · Depends on: Slice 5 · Stories: S-05 · Map: Get a genuine copy

### Outcome

A human-pushed `verifier-v*` tag can publish a static verifier, which a customer can confirm both through provenance and through a reproducible rebuild.

### Acceptance

- **S-05.1** (workflow side): the workflow's assets, evidence gate and manual, non-publishing mode are proven by structural tests.
- **S-05.2** (documented side): the check's exact command is written in the new `docs/adr/` ADR, in two forms that name `release-verifier.yml` as the signer workflow: `--source-ref refs/tags/<tag>` for a published release, and `refs/heads/main` for the operator's manual run.
- **S-05.3** (hash side): a binary with one changed byte fails the SHA-256 comparison.
- **S-05.4**
- **S-05.5**
- **S-05.6**
- **6.1** `docs/adr/` extends the tag rules of ADR-0111 to `verifier-v*`, and `CLAUDE.md`'s rule names those tags.
- **6.2** A step in the `hyalos-os` CI job clones the commit into two temporary directories, builds both in the pinned builder and compares hashes on every `hyalos-os` change. It also builds a copy with one changed byte and expects a different hash (S-05.4.e2).
- **6.3** `release.yml`'s tag filter is `v[0-9]*`, so a `verifier-v*` tag never starts the product release, and `release_gate` asserts it.
- **6.4** `release-evidence.sh` takes an optional job list that defaults to today's, and the verifier's workflow passes `hyalos-os`.
- **6.5** `release-verifier.yml` meets the rules in `design.md` §2, **Release assets**. Its release job uses the exact condition `if: github.event_name == 'push'`, because `release_gate` detects tag-triggered jobs by that line, and `release_gate` runs its job checks over both release workflows. The tag's ancestor check copies `release.yml`'s.
- **6.6** The build script installs `musl-tools` at the exact version apt resolves inside the digest-pinned image, written into the script, mounts each clone at one in-container path, and remaps source paths for the target `x86_64-unknown-linux-musl`. The CI job's `name:` is `hyalos-os`.

### Contracts

`design.md` §2, **Release assets** and **`hyalos-verify` command** (`--version`); ADR-006.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if two builds of one commit differ and the cause cannot be removed (R6, ADR-006). Do not fall back to provenance alone.
- Stop if the workflow could publish without its commit's `hyalos-os` evidence, or could create a tag (pre-mortem P8).
- Stop before any agent step that would start the workflow or create a real attestation, tag or release. Those are the operator's, and GitHub can run a manual workflow only once it is on `main`, so their proof belongs to Slice 7.

### Verify

```bash
cargo test --locked -p xtask --test release_gate
cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify -- s_05 local_6
```

Expected: both pass. The double build in CI reports identical SHA-256 hashes (S-05.4), and the progress file records that run's URL and both hashes. This slice can be `done` without a real attestation, because that side of S-05.1, S-05.2 and S-05.3 is Slice 7's.

### Pattern to follow

- Static binary, provenance and `--verify-tag`: `.github/workflows/release.yml:195` and `:207`.
- Gate tests over workflow text: `crates/xtask/tests/release_gate.rs:1`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Add `--version`, with the commit taken from the build.
2. Make the static build reproducible in a digest-pinned image.
3. Add the workflow, its evidence check and its manual mode, using `docs/operations/releases.md:21` as the model for the `gh attestation verify` command.
4. Extend `release_gate`, then add the ADR and the `CLAUDE.md` rule.

### Files

- Create: `.github/workflows/release-verifier.yml`, a verifier build script modelled on `.github/scripts/build-release-binary.sh`, a new `docs/adr/` ADR
- Modify: `hyalos-os/verify/`, `crates/xtask/tests/release_gate.rs`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/scripts/release-evidence.sh`, `CLAUDE.md`, `docs/adr/README.md`

---

## Slice 7 — Verify a real run alone and publish the guide

Issue: #449 · Depends on: Slice 6 · Stories: S-05, S-06 · Map: Get a genuine copy, Use it for real

### Outcome

A real run's record passes in a clean container against the public log, a tampered copy does not, and a customer guide explains every step and limit.

### Acceptance

- **S-05.1** (real run side): the operator's manual run produces the binary, `SHA256SUMS` and provenance, and a run with failed evidence publishes nothing.
- **S-05.2** (real run side): the `refs/heads/main` form of `gh attestation verify` passes on that binary and fails on one built elsewhere.
- **S-05.3** (real run side): the provenance check fails on a copy with one changed byte.
- **S-06.1**
- **S-06.2**
- **S-06.3**
- **S-06.4**
- **7.1** The guide says that the operator must not be the source of the customer's verifier, that the customer supplies their own notary key, that the operator's public key comes by a separate channel, and that a pass holds only under the keys given.
- **7.2** The real record is `hyalos-os/evidence/out/two-machines-canary/record.json`, with its salt and presentations, the chain in `hyalos-os/evidence/ledger/`, the guest `hyalos-os/guest/digest.wasm`, and its notary key beside it. The operator key is the committed `hyalos-os/evidence/out/two-machines-slice5/operator.pub`, which equals the public key in the stored log entry (`ledger/NNNNN.rekor.json`) of all 12 links. Only links 8 and 10 also carry an `operator` fingerprint. On 2026-11-23, `attest verify` with an empty environment accepted all 40 presentations under the default real trust, naming `api.anthropic.com`. S-06.1 therefore verifies under a key supplied as a file apart from the chain. A real customer gets that key by the separate channel in 7.1. At `c6ae5df` today's audit passes this set offline against a stand-in log of the stored replies. Whether the public log lists exactly these 12 entries under the operator key cannot be checked offline. If it lists any other entry, the audit refuses, and this slice stops under its first public-log stop condition.
- **7.3** A test in `hyalos-verify`, named `local_7_3_...`, collects every `not proven:` string literal in `hyalos-audit`, by exact match, and requires each in the guide, which marks those that appear only under some conditions.
- **7.4** This slice adds `HYALOS_REQUIRE_PUBLIC_LOG` to the `HYALOS_REQUIRE_*` list in `CLAUDE.md`. With it set, an unreachable log fails the test instead of skipping it.
- **7.5** The progress file records the operator's run URL, the binary's SHA-256 and the `gh attestation verify` output.

### Contracts

`design.md` §2, **`hyalos-verify` command**. No new interface.

### Stop conditions

- Stop if a contract this slice depends on has drifted from `context.md` (preflight).
- Stop if no committed real record passes against the public log. A new real run is the operator's step and needs their credentials.
- Do all other work first. Then, if the operator has not yet run the release workflow's manual mode on `main` and reported it, stop with status `blocked`, naming only the S-05.1, S-05.2 and S-05.3 real-run rows. No agent starts it, creates a tag or creates an attestation.
- Stop if a tampered record exits 0.
- Stop if the guide would need to claim something the report does not prove (pre-mortem P7).

### Verify

```bash
cargo build --release --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify
HYALOS_REQUIRE_PUBLIC_LOG=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml -p hyalos-verify -- s_06 local_7
```

Expected: both pass, with no S-06 test skipped.

Dry runs: in a `FROM scratch` Docker image holding only the verifier, the inputs in 7.2, the `/etc/resolv.conf` and `/etc/hosts` a static binary needs to resolve the log, and the TLS roots if the binary does not embed them, with an empty `PATH` and no home directory, run with network and with `--network none` (S-06.1, M3). Follow the guide from a fresh container (S-06.3.e1). Until the operator's first release exists, the guide's download step is exercised with the documented rebuild, and the progress file says so.

### Pattern to follow

- Report limits to list: `audit_report` (`context.md` §2).
- Format reference to extend: `hyalos-os/RECORD.md`.

### Suggested path

Default route. You may deviate if every mandatory item still holds; record why.

1. Run the clean-container check on a committed real record.
2. Add the tampering checks.
3. Write the customer guide, and update `RECORD.md`.
4. Add tests holding the docs' exit codes, size limits and limits list to the code.

### Files

- Create: `docs/operations/verify-a-hyalos-record.md`
- Modify: `hyalos-os/RECORD.md`, `hyalos-os/verify/` tests, `CLAUDE.md`
