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
| "Moving to one lockfile keeps every package version the spike tested" (PRD, Assumptions) | **Differs**: see Blocked on |

## Acceptance

| ID | State |
|---|---|
| S-01.2, S-01.2.e1, S-01.2.e2 | Shown for the partial move: SHA-256 of all 260 evidence files is identical before and after, with none added or missing |
| S-01.1, S-01.3, S-01.4, S-01.5 and their examples | Not shown: the workspace is not finished |
| 1.1, 1.2, 1.3 | Not shown |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Merge the lockfiles, "keeping the version each lockfile resolves today" | Impossible for five cryptographic or TLS crates | The monitor and `attest` lock different semver-compatible versions, and a trial resolve keeps only one of each | **y** |
| `context.md` §2 lists the paths that break | More break: the `source()` helpers, `include_bytes!` in `forgery_tests.rs`, `committed()` and the fixture paths in `live_tests.rs`, the probe's `guest/` and `fixtures/` paths and `attest/tests/canary.rs`'s stub layout | The list was not exhaustive | n |

## Decisions

None. The lockfile choice belongs to the developer.

## Discoveries

Trial resolves ran in a scratch copy with the workspace manifest from ADR-001.

| Crate | Monitor | `attest` | Seeded from `attest` | Seeded from monitor |
|---|---|---|---|---|
| `rustls` | 0.23.46 | 0.23.40 | 0.23.40 | 0.23.46 |
| `rustls-webpki` | 0.103.17 | 0.103.13 | 0.103.13 | 0.103.17 |
| `rustls-pki-types` | 1.16.0 | 1.14.1 | 1.14.1 | 1.16.0 |
| `webpki-roots` (1.x) | 1.0.12 | 1.0.7 | 1.0.7 | 1.0.12 |
| `zeroize` | 1.9.2 | 1.8.2 | 1.8.2 | 1.9.2 |

- Seeding from `attest` keeps every version `attest` locks and moves the monitor on these five crates. In total, 28 package names lose a version from one side.
- Seeding from the monitor also moves `aes`, `blake3`, `rand`, `getrandom`, `crypto-common`, `crypto-bigint` and `cpufeatures` for `attest`. In total, 104 package names lose a version.
- `tlsn` and `wasmi` did not change in either trial.
- Neither trial was built or tested.

## Blocked on

**Question:** one lockfile cannot keep both resolved versions of `rustls`, `rustls-webpki`, `rustls-pki-types`, `webpki-roots` and `zeroize`. Which should the workspace do: accept a named version change for these crates, or keep the monitor and `attest` on separate lockfiles, which changes ADR-001?

**Stop condition:** "Stop if the shared lockfile changes the resolved version of `tlsn`, `wasmi` or any cryptographic crate (R1)."

**Partial work:** the tree is moved by `git mv` to the layout in `design.md` §1. The workspace manifest, lockfile, path fixes, CI, `deny.toml`, CODEOWNERS, ADR and `CLAUDE.md` wording are not done, so this branch does not build as it stands.
