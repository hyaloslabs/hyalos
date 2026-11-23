# PRD — hyalOS: a standalone verifier a customer can run alone

## Problem

The parent's promise is a record that a stranger can check without trusting the operator. Four things stand in the way today:

1. **There is no program a customer can get.** The check is the `audit` subcommand of the spike's monitor, so a customer must build the spike, including the code that runs tasks.
2. **It depends on tools outside it.** It runs an external `openssl` for every signature and a separate `attest` program for every attestation, and `attest` locks 289 packages.
3. **A record does not name what its replay depends on.** Replay compares interpreter-counted fuel byte for byte, but the record does not say which interpreter settings produced it. An interpreter change could make an honest record fail with no explanation.
4. **Nothing shows a copy is genuine.** A verifier the operator hands over proves nothing.

## Users and jobs

- **Customer:** an organisation that receives automated work and runs its own notary.
- **Third-party auditor:** someone the customer asks to check a record, with no account on any Hyalos system.
- **Maintainer:** the person who builds and releases hyalOS.

**Jobs.**

- Customer: "When an operator hands me a run's record, I want to check it with a tool the operator did not supply, so I can rely on it without trusting them."
- Third-party auditor: "When I am given a record and public keys, I want a clear pass or refusal and a list of what it does not prove, so I can state exactly what I checked."
- Maintainer: "When the interpreter or format changes, I want old records to keep their meaning, so an upgrade never breaks a customer's evidence."

## Goals

- G1. hyalOS lives in its own workspace at `hyalos-os/`, tested in CI, with no change in behaviour.
- G2. Every new record names its execution profile, and every older record still replays.
- G3. A customer can check a record with one program that holds no task-running, notary-service or operator-signing code.
- G4. The verifier needs no external program.
- G5. A customer can confirm their copy is genuine without any Hyalos tool and without trusting the operator.
- G6. A real run's record passes in a clean environment, and a tampered one does not.

## Non-goals

- Checking the Bitcoin timestamp.
- Keeping task data private during verification. The verifier reads the record's inputs and questions.
- Other forged request shapes (#441) and the zero-knowledge proof of the hidden bytes (#442).
- Closing any limit the audit reports today. The verifier reports the same limits.
- Python guests (#438) or any change to the four guest calls.
- Hardening the monitor, confining it, or the independent review (`hyalos-kernel-core`).
- Joining v1's engine (`hyalos-kernel-engine`). No `core/` crate depends on `hyalos-os/`.
- Platforms other than x86-64 Linux for the first release.
- Publishing a release. The operator pushes the tag, and no slice or agent does.

## Success measures

- M1. All 285 tests from the baseline still pass after the move, and they run in CI.
- M2. For every committed record, the verifier's report is byte-identical to the audit's.
- M3. With `PATH` empty and no home directory, the verifier passes a real version 3 record.
- M4. Every forgery in the carried-over corpus is refused, with none reported as a pass.
- M5. Two builds from one tag in different directories give the same SHA-256.
- M6. The verifier's dependency tree contains no `tlsn` prover, `kvm-*` or notary-service package.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | A 5-answer record, with a stored stand-in log, is verified in under 10 seconds on the development machine |
| Reliability | Any error, timeout or unreadable input ends with no pass. Exit 0 only follows a completed audit |
| Security | Hostile records, logs and presentations cannot produce a pass. Input sizes are bounded. The crate forbids `unsafe` |
| Privacy | The only data sent anywhere is the operator's public key and entry IDs, to the public log. Task data never leaves the machine |
| Accessibility | Plain text on standard output, with no color or terminal control codes |
| Observability | none: a one-shot command whose report and exit code are its whole output |
| Compatibility / portability | Static x86-64 Linux binary. Reads record versions 1 to 4 |
| Maintainability | One definition each for the profile, the record types and the report. The verifier is at most 3,000 non-blank, non-comment lines of our own code |

## Assumptions

| Assumption | How to validate | If false |
|---|---|---|
| `tlsn-attestation` and `tlsn-core` can verify a presentation without the `tlsn` crate | The slice that moves the check builds it and passes every presentation fixture | Stop. The verifier links `tlsn` and the dependency measure is revised |
| A pure-Rust P-256 check accepts exactly what `openssl` accepts on the log's real signatures | The same slice compares both over every stored signature and a tampered set | Stop before replacing `openssl` |
| A static musl build from one tag is reproducible across directories | The release slice builds twice and compares hashes | Stop. Reproducibility is reduced to provenance only, as a developer decision |
| Moving to one lockfile keeps every package version the spike tested | The move slice compares the resolved versions | Stop on any change to `tlsn`, `wasmi` or a cryptographic crate |
| Rekor's index lists every entry under a key | The chain check already refuses a list that differs from the chain | A list that differs from the chain is refused. A list over the verifier's 10,000-entry limit is inconclusive, so it never accuses an honest operator |

## Story map

| Move and name | Check a record | Stand alone | Get a genuine copy | Use it for real |
|---|---|---|---|---|
| S-01, S-02 | S-03 | S-04 | S-05 | S-06 |

## Stories and acceptance criteria

### S-01 hyalOS has its own workspace

As a maintainer, I want the spike moved into a tested workspace with no change in behaviour, so that later work builds on code CI actually runs.

- **S-01.1** Given the moved code, when its test commands run, then at least the baseline's 285 tests pass and none fail.
- **S-01.2** Given the move, when the committed evidence files are compared with their originals, then every one is byte-identical.
- **S-01.3** Given a pull request that breaks a `hyalos-os/` test, when CI runs, then the required check fails.
- **S-01.4** Given the move, when the main workspace's checks run, then they pass and the trusted core's line count does not change.
- **S-01.5** Given the move, when the programs report their release identifier, then it is the one the spike reported.

### S-02 A record names its execution profile

As a maintainer, I want each record to name the profile it ran under, so that replay stays meaningful when the interpreter changes.

- **S-02.1** Given both backends, when they configure the interpreter, then both take every setting and limit from one definition.
- **S-02.2** Given a new run, when its record is written, then it is version 4, names profile `hyalos.profile/1`, and its task identity covers that profile.
- **S-02.3** Given a version 1, 2 or 3 record, when it is replayed, then it runs under profile 1 with its bytes unchanged.
- **S-02.4** Given a version 4 record that names an unknown or missing profile, when it is read, then it is refused and the refusal says why.
- **S-02.5** Given one guest and inputs, when run on both backends, then the two records are byte-identical.
- **S-02.6** Given a version 4 record relabelled as another version or profile, when it is audited, then it is refused.

### S-03 A customer checks a record with one program

As a customer, I want one program that gives a pass or refusal, so that I do not need the code that runs tasks.

- **S-03.1** Given any committed record that the audit passes, when the verifier checks it with the same keys, then its report is byte-identical to the audit's report captured before the code was extracted, and it exits 0.
- **S-03.2** Given any forgery in the carried-over corpus, when the verifier checks it, then it exits 1 and prints no line starting `audit passed` or `legacy audit passed`.
- **S-03.3** Given a check that cannot finish, when the verifier stops, then it exits 2, says what could not be checked, and prints no pass.
- **S-03.4** Given the verifier's build, when its dependency tree is listed, then it holds no KVM, notary-service, operator-signing or live task-running code. Replay is the only way it runs a guest.
- **S-03.5** Given any run, when the verifier finishes, then it has written no file and read nothing under the home directory.
- **S-03.6** Given an input over its size limit, when the verifier reads it, then it stops with exit 1 before parsing it.
- **S-03.7** Given a release build, when any stand-in trust setting is present in the environment, then the setting has no effect.

### S-04 The verifier needs nothing else installed

As a customer, I want the verifier to run with no other tools, so that a substituted `openssl` or `attest` cannot change its answer.

- **S-04.1** Given an empty `PATH`, when the verifier checks a valid record, then it passes.
- **S-04.2** Given every stored presentation, valid and altered, when the in-process check and the `attest verify` program both judge it, then they agree on the verdict and on every reported field.
- **S-04.3** Given every stored log and link signature, valid and altered, when the in-process check and `openssl` both judge it, then they agree.
- **S-04.4** Given the verifier's options, when `--verify` is passed, then it is rejected as an unknown option.

### S-05 A customer can confirm a genuine copy

As a customer, I want to confirm where my verifier came from, so that I am not trusting a binary the operator handed me.

- **S-05.1** Given a verifier release, when it is published, then it holds a static binary, its SHA-256 and build provenance.
- **S-05.2** Given a released binary, when the customer runs the documented provenance check, then it passes using only GitHub's CLI and no Hyalos tool.
- **S-05.3** Given a binary altered by one byte, when it is checked either way, then the check fails.
- **S-05.4** Given the tagged source, when the customer rebuilds it with the documented command in any directory, then the result has the published SHA-256.
- **S-05.5** Given the release workflow, when it runs, then it publishes only for a tag a human pushed, and creates no tag.
- **S-05.6** Given the verifier, when run with `--version`, then it prints its version, source commit and the record versions it reads.

### S-06 A real run verifies alone

As a customer, I want proof this works on a real run, so that the promise holds outside the test suite.

- **S-06.1** Given a clean environment holding only the verifier, a real record with its salt and presentations, its link chain, its guest and public keys, when it runs against the public log, then it passes.
- **S-06.2** Given that record with one byte changed in its transcript, presentation or a link, when the verifier checks it, then it does not pass.
- **S-06.3** Given the customer guide, when a reader follows it from a fresh machine, then they can get the verifier, confirm it and check a record, and it lists every limit a pass does not prove.
- **S-06.4** Given `RECORD.md`, when compared with the verifier's behaviour, then it describes version 4, the profile, the options, the exit codes and the size limits.

## Rules and examples

### S-01 hyalOS has its own workspace

Rule: moving the code changes no behaviour and no evidence.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.1.e1 | The moved tree, with `attest` built first | Every test command runs | 285 or more pass, 0 fail |
| S-01.1.e2 | The moved tree, with `attest` not built | The monitor's tests run | They fail naming the missing program, and do not silently skip |
| S-01.2.e1 | `out/live3.json`, at `hyalos-os/evidence/out/live3.json` after the move | Hashed before and after | Same SHA-256 |
| S-01.2.e2 | Any file under the moved `ledger/` or `out/` | Hashed before and after | Same SHA-256, with none added or missing |
| S-01.3.e1 | A pull request changing a `hyalos-os/` source file so a test fails | CI runs | The required check is red |
| S-01.3.e2 | A pull request changing only `docs/` | CI runs | The `hyalos-os` job is skipped and the required check is green |
| S-01.4.e1 | The moved tree | `cargo xtask lines` runs | 28,594 lines, or the count `main` reports at the slice's base |
| S-01.4.e2 | A `core/` crate that adds a `hyalos-os/` path dependency | `cargo xtask layout` runs | It fails |
| S-01.5.e1 | `attest` after the move | Its `RELEASE` is read | `v0.1.0-beta.1/0.2.0` |
| S-01.5.e2 | A notary built before the move and a client built after | They open a session | No `release` refusal |

### S-02 A record names its execution profile

Rule: the profile is one definition, part of a new record's identity, and implied for old records.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.1.e1 | The process backend and the kernel image | Their settings are compared by a test | They are the same definition |
| S-02.1.e2 | A guest using floats | Run on either backend | Refused, as today |
| S-02.2.e1 | A new run | Its record is read | `version` 4 and `profile` `hyalos.profile/1` |
| S-02.2.e2 | One guest and inputs under two profile names | Task identities are computed | They differ |
| S-02.3.e1 | `out/live3.json` (version 1), `out/two-machines-record/record.json` (version 2) and `out/two-machines-canary/record.json` (version 3) | Replayed | Identical |
| S-02.3.e2 | A version 3 record that adds a `profile` field | Read | Refused as an unknown field |
| S-02.4.e1 | A version 4 record naming `hyalos.profile/9` | Read | Refused, naming the unknown profile |
| S-02.4.e2 | A version 4 record with no `profile` | Read | Refused |
| S-02.5.e1 | The example guest and fixture inputs | Run in process and on KVM | Byte-identical records |
| S-02.5.e2 | A host with no KVM | The comparison runs | Reported as skipped, and the `hyalos-os` CI job does not skip it |
| S-02.6.e1 | A version 4 record relabelled version 3 | Audited | Refused |
| S-02.6.e2 | A version 4 record with its `profile` changed | Audited | Refused, because the task and commitment no longer match |

### S-03 A customer checks a record with one program

Rule: exit 0 means a completed audit passed, and nothing else does.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.1.e1 | The real version 3 canary record and a stand-in log of its stored replies | Verifier and audit both run | Identical stdout, exit 0 |
| S-03.1.e2 | A version 1 record without `--legacy-v1` | Verifier runs | Exit 1, naming the option, as the audit refuses |
| S-03.2.e1 | A record whose link has a second result | Verifier runs | Exit 1, no pass line |
| S-03.2.e2 | A presentation from an untrusted notary | Verifier runs | Exit 1, naming the answer |
| S-03.3.e1 | The public log is unreachable | Verifier runs | Exit 2, no pass line |
| S-03.3.e2 | A named key file is missing | Verifier runs | Exit 2, naming the file's role |
| S-03.4.e1 | The verifier's normal dependency tree | Listed | No `kvm-ioctls`, `kvm-bindings`, `tlsn`, `tokio-rustls` or `rcgen` |
| S-03.4.e2 | The verifier's source | Searched | It has no use of the monitor's `Live`, KVM backend, or link-signing code |
| S-03.5.e1 | `HOME` set to a directory the process has no permission to read | Verifier runs | It completes, and no new file appears in its working directory |
| S-03.5.e2 | A read-only working directory and no `/tmp` access | Verifier runs | It still completes |
| S-03.6.e1 | A presentation of 1 MiB exactly | Verifier reads it | It proceeds to the check |
| S-03.6.e2 | A presentation of 1 MiB plus one byte | Verifier reads it | Exit 1, naming the limit, before parsing |
| S-03.7.e1 | A release build with `HYALOS_TRUST_LOG_DIR` and `HYALOS_TRUST_LOG_KEY` set | Verifier runs | It uses the real log's pinned key |
| S-03.7.e2 | A release build with `HYALOS_TRUST_ROOT` set | Verifier checks a stand-in provider's presentation | Exit 1 |

### S-04 The verifier needs nothing else installed

Rule: every check runs in the verifier's own process and agrees with the tool it replaces.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.1.e1 | `PATH` empty | Verifier checks the real record with a stand-in log | Exit 0 |
| S-04.1.e2 | A decoy `openssl` that always succeeds, first on `PATH` | Verifier checks a forged signature | Exit 1, and the decoy never ran |
| S-04.2.e1 | Every committed `.presentation` | Both judge it | The same fields |
| S-04.2.e2 | Each presentation with one byte changed | Both judge it | Both refuse |
| S-04.3.e1 | Every stored log and link signature | Both judge it | Both accept |
| S-04.3.e2 | Each with one bit flipped | Both judge it | Both refuse |
| S-04.4.e1 | `--verify /bin/true` | Verifier starts | Exit 2, unknown option |
| S-04.4.e2 | No `--verify` | Verifier starts | It runs normally |

### S-05 A customer can confirm a genuine copy

Rule: a copy is confirmed by evidence the operator cannot produce.

| ID | Given | When | Then |
|---|---|---|---|
| S-05.1.e1 | A pushed `verifier-v*` tag with green evidence | The workflow runs | The release holds the binary, `SHA256SUMS` and provenance |
| S-05.1.e2 | The evidence job failed | The workflow runs | Nothing is published |
| S-05.2.e1 | The published binary | `gh attestation verify` runs against this repository | It passes |
| S-05.2.e2 | A binary built by someone else | The same command runs | It fails |
| S-05.3.e1 | The binary with one byte changed | Its SHA-256 is compared | It differs |
| S-05.3.e2 | The binary with one byte changed | The provenance check runs | It fails |
| S-05.4.e1 | The same tag, built in two different directories | The hashes are compared | Identical |
| S-05.4.e2 | A source tree with one changed byte in the verifier | Built the same way | A different hash |
| S-05.5.e1 | The workflow files | `release_gate` checks them | None creates a tag, and every release uses `--verify-tag` |
| S-05.5.e2 | A tag not on `main` | The workflow runs | It stops before publishing |
| S-05.6.e1 | A release build | `--version` runs | Version, 40-hex commit, `records 1-4` and `profiles hyalos.profile/1` |
| S-05.6.e2 | A build outside a git checkout | `--version` runs | It says the commit is unknown and does not invent one |

### S-06 A real run verifies alone

Rule: the promise is demonstrated on real evidence, with its limits stated.

| ID | Given | When | Then |
|---|---|---|---|
| S-06.1.e1 | A container with only the verifier, the record with its salt and presentations, its link chain, its guest and public keys | It runs with network access to the log | Exit 0, naming the public log |
| S-06.1.e2 | The same container with no network | It runs | Exit 2, no pass |
| S-06.2.e1 | One changed byte in the transcript | Verifier runs | No pass |
| S-06.2.e2 | One changed byte in a presentation, or in a link | Verifier runs | No pass |
| S-06.3.e1 | The guide's steps, from a fresh container | Followed in order | The record is verified |
| S-06.3.e2 | The guide's list of limits | Compared with the report's `not proven` lines | Every line appears |
| S-06.4.e1 | The exit codes in `RECORD.md` | Compared with the verifier's tests | They match |
| S-06.4.e2 | The size limits in `RECORD.md` | Compared with the verifier's constants | They match |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| False pass | A forged record | Refused | `refused: <the check>`, exit 1 | S-03.2 |
| Log unreachable | Network or Rekor outage | Inconclusive | `inconclusive: <what>`, exit 2 | S-03.3 |
| Hostile large input | A huge file | Stopped before parsing | `refused: <file> is over its limit`, exit 1 | S-03.6 |
| Substituted tool | A decoy `openssl` on `PATH` | Never run | Normal verdict | S-04.1 |
| Stand-in used in production | Trust variables set | Ignored by a release build | Normal verdict against the real log | S-03.7 |
| Unknown record or profile | A newer record | Refused | The version or profile named, exit 1 | S-02.4 |
| Interpreter update changes fuel | A dependency bump | A new profile is required, and old records keep replaying | A test fails before merge | S-02.3 |
| Tampered binary | Operator hands over a modified verifier | The hash or provenance check fails | The check's failure | S-05.3 |
| Evidence changed by the move | A file altered in the move | The hash comparison fails | Slice 1 stops | S-01.2 |
