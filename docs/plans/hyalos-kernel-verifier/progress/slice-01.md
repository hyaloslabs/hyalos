# Slice 1 progress

- **Status:** blocked
- **Issue:** #443
- **PR:** #452, held as a draft
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
| `hyalos-os/deny.toml` "allows exactly" the `tlsn` git source (`plan.md`, Slice 1 Contracts) | **Differs**: resolved by the developer's decision below |

## Acceptance

| ID | How it was shown |
|---|---|
| S-01.1, S-01.1.e1 | Locally, `cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace`, with `attest` built first: 285 passed, 0 failed, 0 ignored. **Not yet green in CI**: see Blocked on |
| S-01.1.e2 | With the `attest` program moved aside, the monitor's tests fail: 83 failed, and 83 name `attest is not built` |
| S-01.2, S-01.2.e1, S-01.2.e2 | SHA-256 manifest of all 260 evidence files at the base equals the moved files, `out/live3.json` included, with none added or missing |
| S-01.3, S-01.3.e1 | **Not fully shown.** Real `hyalos-os` test failures turned `build, lint, test` red in runs [45244084495](https://github.com/hyaloslabs/hyalos/actions/runs/45244084495), [45246001427](https://github.com/hyaloslabs/hyalos/actions/runs/45246001427) and [45251953860](https://github.com/hyaloslabs/hyalos/actions/runs/45251953860). The deliberately failing test never ran in them, because Cargo stopped at the earlier failure |
| S-01.3.e2 | Dry run of the `ci.yml` rule: a `docs/` change gives `hyalos_os=false`, and the gate accepts `skipped` only with `false`. A skip with `true` or an unset output fails |
| S-01.4 | `cargo fmt --check`, clippy, `cargo xtask layout`, `lines`, `docs` and `cargo deny check` pass. `cargo test --workspace --locked` passes 1,018 tests with a short target directory |
| S-01.4.e1 | `cargo xtask lines`: 28,594, the base's count |
| S-01.4.e2 | A temporary `task-kernel` path dependency in `core/types` makes `cargo xtask layout` exit 1, naming the crate |
| S-01.5, S-01.5.e1 | The `attest` unit test pins `RELEASE` to `v0.1.0-beta.1/<version>`, and the crate is still 0.2.0 |
| S-01.5.e2 | A notary built from the base commit served a client built after the move: `s_01_2_e1...` passed against it, with no `release` refusal |
| 1.1 | `spikes/task-kernel` is gone, and `git grep -n 'spikes/task-kernel' -- ':!docs/plans'` prints nothing |
| 1.2 | `ci.yml` adds the output, the job and the gate. The classification and gate dry runs above cover both cases |
| 1.3 | `docs/adr/0130-hyalos-os-is-its-own-workspace.md`, with its README row |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Merge the lockfiles, "keeping the version each lockfile resolves today" | `rustls`, `rustls-webpki`, `rustls-pki-types`, `webpki-roots` and `zeroize` take the monitor's newer versions for `attest` | Cargo keeps one semver-compatible version, and the developer chose this | y, approved |
| The five crates above | `zeroize_derive` also moves, 1.4.3 to 1.5.0 | `zeroize` 1.9.2 requires it, and pinning 1.4.3 fails to resolve | y, follows from the approved `zeroize` version |
| Other shared crates | The monitor takes `attest`'s versions of 23 non-cryptographic crates, such as `serde`, `libc` and `log` | The lockfile is seeded from `attest` | n |
| `cargo deny --manifest-path ... check --config ...` | `--config` must come before `check` | cargo-deny 0.20.2 takes it as a global option | n |
| `context.md` §2 lists the paths that break | More break: the `source()` helpers, `include_bytes!` in `forgery_tests.rs`, `committed()` and the fixture paths in `live_tests.rs`, the probe's paths and `attest/tests/canary.rs`'s stub layout | The list was not exhaustive | n |
| `hyalos-os/deny.toml` allows exactly the `tlsn` git source | It allows TLSNotary's four repositories, adds two licences, states the licence of 26 crates and accepts three advisories | The audit cannot pass otherwise, and the developer chose this | y, approved |
| Keep `attest`'s other locked versions | `anyhow`, `crossbeam-epoch`, `crypto-bigint` 0.7 (0.7.5 to 0.7.11), `ctutils`, `tokio` and `webpki-root-certs` move to fixed versions | Published advisories, approved by the developer. `crypto-bigint` 0.7.6 was yanked, so 0.7.11 is used | y, approved |
| Moved tests are unchanged except for paths | `adr_014_a_session_left...` now waits for the notary to end the first session | It raced the notary's one-session rule: 2 of 40 runs failed under load on the base and on this branch, and it failed in CI. With the wait, 100 of 100 passed under load. Only the test changed | n |

## Decisions

- **Lockfile (developer, 2026-11-23):** use the newer versions of the five crates and keep every other version `attest` locks. `tlsn` and `wasmi` are unchanged.
- **Dependency audit (developer, 2026-11-23):** apply the compatible advisory fixes, allow TLSNotary's sources and licences, and record reasoned exceptions for `bincode`, `rustls-pemfile` and `crypto-bigint` 0.5.5, with #451 to assess the last against notary signing before any verifier release.

## Discoveries

- **#451 must close before a verifier release.** The timing advisory on `crypto-bigint` 0.5.5 is held by TLSNotary's `k256` and `p256`, and its reach into notary signing is unassessed.
- `aes` 0.9.0 and `der` 0.8.0 are yanked on crates.io and held by TLSNotary. They are unchanged, and `cargo deny` reports them as warnings.
- 12 `broker_channel` tests in the main workspace fail with `Io(InvalidInput)` when the target directory is on a deep path, at the base commit too. They pass with a short `CARGO_TARGET_DIR`.
- `context.md`'s list of paths that break was incomplete, and Slice 2 onward should read paths under `hyalos-os/`.
- `s_01_3_e2_a_link_cut_at_any_point...` fails 12 of 12 runs on one busy CPU, on the base and on this branch alike, and failed once in CI. The notary logs `none 0 0 refused deadline`, about 30 seconds after the client was killed. The cause inside the notary is not established.

## Blocked on

**Question:** `s_01_3_e2_a_link_cut_at_any_point_is_never_logged_as_a_refused_server` requires `refused dropped` or `attested`, but on a slow machine a killed client can leave the notary logging `refused deadline` after about 30 seconds. Should the test also accept `refused deadline`, should the notary be changed to detect the drop sooner, or should something else happen?

**Stop condition:** "Stop if a test needs a behaviour change to pass after the move." Accepting a new outcome would weaken a security test, and changing the notary would change behaviour, so neither is this slice's to choose.

**Partial work:** everything in Acceptance is in place except a green CI run and a clean S-01.3.e1 proof. The temporary failing test is removed.
