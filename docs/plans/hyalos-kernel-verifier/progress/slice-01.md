# Slice 1 progress

- **Status:** blocked
- **Issue:** #443
- **PR:** none (no PR is opened)
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `02f66d1`.

| Citation | Result |
|---|---|
| `spikes/task-kernel`, 302 tracked files | Holds |
| Four separate builds and their manifests (`context.md` §1) | Holds |
| `kernel/src/live_tests.rs:21`, `attest/tests/common/mod.rs:169`, `kernel/tests/audit_cli.rs:41` | Holds |
| `attest/probes/two_machines.sh:7`, `:13`, `:14` | Holds |
| `attest/build.rs` reads the `tlsn = ` line | Holds |
| `.github/workflows/ci.yml:48`, `:59`, `:60` and the required check | Holds |
| "Moving to one lockfile keeps every package version the spike tested" (PRD, Assumptions) | **Differs**: resolved by the developer's decision below |
| `hyalos-os/deny.toml` "allows exactly" the `tlsn` git source (`plan.md`, Slice 1 Contracts) | **Differs**: see Blocked on |

## Acceptance

| ID | State |
|---|---|
| S-01.1, S-01.1.e1 | Shown on this branch: 285 passed, 0 failed, 0 ignored, with `attest` built first |
| S-01.1.e2 | Not shown |
| S-01.2, S-01.2.e1, S-01.2.e2 | Shown: SHA-256 of all 260 evidence files is identical before and after, with none added or missing |
| S-01.3, S-01.3.e1, S-01.3.e2 | Not shown: needs a pull request |
| S-01.4, S-01.4.e1, S-01.4.e2 | Not shown |
| S-01.5, S-01.5.e1 | Partly shown: the `attest` unit test that pins `RELEASE` to `v0.1.0-beta.1/<version>` passes |
| S-01.5.e2 | Not shown |
| 1.1 | Not shown: the final `git grep` has not been run |
| 1.2 | Written in `ci.yml`, not run |
| 1.3 | Not done: the ADR is not written |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Merge the lockfiles, "keeping the version each lockfile resolves today" | `rustls`, `rustls-webpki`, `rustls-pki-types`, `webpki-roots` and `zeroize` take the monitor's newer versions for `attest` | Cargo keeps one semver-compatible version, and the developer chose this | y, approved |
| The five crates above | `zeroize_derive` also moves, 1.4.3 to 1.5.0 | `zeroize` 1.9.2 requires it, and pinning 1.4.3 fails to resolve | y, follows from the approved `zeroize` version |
| Other shared crates | The monitor takes `attest`'s versions of 23 non-cryptographic crates, such as `serde`, `libc` and `log` | The lockfile is seeded from `attest` | n |
| `cargo deny --manifest-path ... check --config ...` | `--config` must come before `check` | cargo-deny 0.20.2 takes it as a global option | n |
| `context.md` §2 lists the paths that break | More break: the `source()` helpers, `include_bytes!` in `forgery_tests.rs`, `committed()` and the fixture paths in `live_tests.rs`, the probe's paths and `attest/tests/canary.rs`'s stub layout | The list was not exhaustive | n |
| `hyalos-os/deny.toml` allows exactly the `tlsn` git source and `cargo deny` passes | It fails on three more git sources, licences and advisories | See Blocked on | **y** |

## Decisions

- **Lockfile (developer, 2026-11-23):** use the newer versions of the five crates and keep every other version `attest` locks. `tlsn` and `wasmi` are unchanged.

## Discoveries

`cargo deny` findings on the merged lockfile. Every advisory is on a version `attest` already locked at the base commit, so the move did not introduce it, but CI has never run this audit on the spike.

| Advisory | Crate | Kind | Fix |
|---|---|---|---|
| RUSTSEC-2026-0387 | `crypto-bigint` 0.7.5 | Timing | 0.7.6 resolves in a dry run, and moves `ctutils` to 0.4.8 |
| RUSTSEC-2026-0387 | `crypto-bigint` 0.5.5 | Timing | **None in range.** Held by `elliptic-curve` 0.13 under `k256` and `p256`, which `tlsn-attestation` uses |
| RUSTSEC-2026-0385 | `ctutils` 0.4.2 | Timing | 0.4.4 resolves in a dry run |
| RUSTSEC-2026-0383 | `webpki-root-certs` 1.0.7 | Distrusted roots | 1.0.10 resolves in a dry run |
| RUSTSEC-2026-0204 | `crossbeam-epoch` 0.9.18 | Vulnerability | 0.9.20 resolves in a dry run |
| RUSTSEC-2026-0386 | `tokio` 1.52.1 | Unsound | 1.53.3 resolves in a dry run |
| RUSTSEC-2026-0190 | `anyhow` 1.0.102 | Unsound | 1.0.103 resolves in a dry run |
| RUSTSEC-2025-0141 | `bincode` 1.3.3 | Unmaintained | None. A direct dependency of `attest` |
| RUSTSEC-2025-0134 | `rustls-pemfile` 1.0.4 | Unmaintained | None. Held by `tlsn-tls-core` |

- Other git sources TLSNotary needs: `tlsnotary/mpz2`, `tlsnotary/tlsn-utils` and `tlsnotary/rs-merkle`.
- Licences: `arrayref` and `tiny-keccak` are rejected by the copied policy, and 25 TLSNotary git crates declare no licence.
- Whether the `crypto-bigint` 0.5.5 timing advisory reaches the notary's secp256k1 signing has not been assessed.
- None of the dry-run updates has been applied, built or tested.

## Blocked on

**Question:** `cargo deny` cannot pass under the slice's contract. Should `hyalos-os/deny.toml` allow TLSNotary's three other git sources and its licences, which crates should be updated, and how should the advisories with no available fix be handled, above all `crypto-bigint` 0.5.5?

**Stop conditions:** README rule 6 (the contract "Allows exactly the git source in `attest/Cargo.toml`" cannot hold with Verify's passing `deny`), and "Stop if the shared lockfile changes the resolved version of `tlsn`, `wasmi` or any cryptographic crate (R1)" for the `crypto-bigint` and `ctutils` fixes.

**Partial work:** the tree is moved, the workspace and merged lockfile build, paths are fixed, and all 285 tests pass. The `ci.yml` job, CODEOWNERS and a failing `hyalos-os/deny.toml` are in place. The ADR and the `CLAUDE.md` and `docs/README.md` wording are not done.
