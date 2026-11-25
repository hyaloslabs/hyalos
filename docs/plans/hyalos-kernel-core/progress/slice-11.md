# Slice 11: Third-party scope and advisories

| | |
|---|---|
| **Status** | done |
| **Issue** | #467 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-24 |

## Preflight

First run against the branch's base, `812c403`. Rechecked on resuming, after merging `origin/main` at `705cecb` (#488).

| Citation | Holds? |
|---|---|
| `context.md` §2: `wasmi =2.0.2` in `hyalos-os/monitor/Cargo.toml` and `hyalos-os/kernel-image/Cargo.toml` (and `hyalos-os/audit/Cargo.toml`) | First run: held, but 2.0.2 was yanked and GHSA-qrr7-2g7f-r9rg named it (the block). On resuming: **differs, by the developer's decision**: #488 pins `=2.0.3` in all three, with `wasmi_core`, `wasmi_ir` and `wasmi_collections` at 2.0.3 in `hyalos-os/Cargo.lock` and `hyalos-os/kernel-image/Cargo.lock` (repository ADR-0134). See Deviations |
| `context.md` §2: `kvm-ioctls =0.25.1`, `kvm-bindings =0.14.2` in `hyalos-os/monitor/Cargo.toml` | Holds |
| `context.md` §2: TLSNotary at tag `v0.1.0-beta.1`, `hyalos-os/attest/Cargo.toml:12` | Holds. Locked at commit `40a069565a2eaecbb5e691dbc634be20e8498daf` for `tlsn`, `tlsn-core` and `tlsn-attestation` |
| `context.md` §2: the upstream audit status of `wasmi` and `tlsn` (searched 2026-11-23) | Holds on repeat searches on 2026-11-24: no audit of `wasmi` 2.0.x and none of `tlsn` |
| ADR-008: `hyalos-os/attest/Cargo.toml:23-24`, `rcgen` and `tokio-rustls` with `ring` | Holds |
| ADR-008: the certificate check at `hyalos-os/attest/src/lib.rs:476` | Differs: the line moved. The check is the `Pin` verifier (`impl ServerCertVerifier for Pin`, line 690) in the same file. Found by symbol, not a contract change |
| `context.md` §3: dependency policy `deny.toml:50-54` and `hyalos-os/deny.toml` | Holds (`[sources]` at 50-54; `hyalos-os/deny.toml` has the three exceptions RUSTSEC-2026-0387, -2025-0141, -2025-0134) |
| Slice 11 pattern: the `hyalos-os` job at `.github/workflows/ci.yml:218` | Differs: the job starts at line 221. Found by name |
| Slice 11 files: `crates/xtask/src/main.rs` | Holds. Slice 10 added `crates/xtask/src/hyalos.rs`, the pattern for a check in a file of its own |
| `design.md` §2 "Review scope" and **Modifier: integration** | Hold as written |

## Acceptance

| ID | How it was shown |
|---|---|
| S-06.1 | `hyalos-os/review/scope.toml`: 43 rows, each with version, `upstream_audit`, part, position and reason, for `wasmi` (and `wasmi_core`, `wasmi_ir`, `wasmi_collections`), `kvm-ioctls`, `kvm-bindings`, `seccompiler`, `landlock`, `tlsn`, `tlsn-core`, `tlsn-attestation`, `sha2`, `p256`, `k256`, `rustls`, `tokio-rustls`, `ring`, `rcgen`, `ureq`, `bincode`, and every other direct dependency. Test `core_s_06_1_the_scope_lists_every_component_the_claim_rests_on`; `cargo xtask review-scope` prints `43 rows agree with 3 lockfiles` |
| S-06.1.e1 | Both `wasmi` rows say the public audits cover `0.31.0` (SRLabs) and `0.36.0`-`0.38.0` (Runtime Verification) and `Neither covers 2.0.3`. Test `core_s_06_1_e1_the_wasmi_row_names_the_audited_versions_and_not_this_one`. The example names 2.0.2; the pin is now 2.0.3 (Deviations) |
| S-06.1.e2 | Test `core_s_06_1_e2_a_row_with_no_reason_fails` (a missing and a blank `reason` each fail, naming the row). Also `core_s_06_1_unknown_values_and_unreadable_lines_fail` and `core_s_06_1_split_components_need_both_rows_and_their_positions` |
| S-06.2 | `cargo xtask review-scope` in CI's `hyalos-os` job (`.github/workflows/ci.yml`) and in the daily job. Tests `core_s_06_2_a_moved_tlsn_tag_or_commit_fails`, `core_s_06_2_an_unscoped_direct_dependency_fails`, `core_s_06_2_a_listed_component_in_no_lockfile_fails`, `core_s_06_2_an_unreadable_lockfile_fails`, `core_s_06_2_the_repository_scope_agrees_with_the_lockfiles` |
| S-06.2.e1 | Test `core_s_06_2_e1_versions_equal_to_the_scope_pass`; `cargo xtask review-scope` exits 0 on the branch |
| S-06.2.e2 | Test `core_s_06_2_e2_wasmi_moved_without_a_scope_change_fails` (a fixture, and the real lockfiles with `wasmi` set to 2.0.4). Dry run: `wasmi` set to 2.0.4 in `hyalos-os/kernel-image/Cargo.lock`, then `cargo xtask review-scope` exited 1 with `wasmi 2.0.4 is locked but has no row; its rows name another version, so read them again`; reverted with `git checkout` |
| S-06.3 | RustSec: `cargo deny` in CI's `hyalos-os` job, and on all three lockfiles in `.github/workflows/hyalos-advisories.yml` (daily and on dispatch, read-only), after a canary that must report RUSTSEC-2021-0003. GitHub: `cargo xtask advisories` on the feeds of `wasmi-labs/wasmi` and `tlsnotary/tlsn` in the same job. Tests `core_s_06_3_a_feed_that_cannot_be_read_fails`, `core_s_06_3_ranges_follow_semver`, `core_s_06_3_a_withdrawn_advisory_or_another_ecosystem_is_not_a_finding` |
| S-06.3.e1 | Test `core_s_06_3_e1_no_advisory_affects_the_pinned_versions` (the saved feeds of 2026-11-24 against the real lockfiles). Live: `cargo deny --manifest-path hyalos-os/Cargo.toml --config hyalos-os/deny.toml check advisories` printed `advisories ok` (exit 0); the same for the kernel image's and the guest's lockfiles; `cargo xtask advisories` on freshly read feeds printed `no published advisory of wasmi-labs/wasmi or tlsnotary/tlsn affects a pinned version` (exit 0) |
| S-06.3.e2 | Test `core_s_06_3_e2_an_advisory_against_a_pinned_crate_fails_naming_it` (the saved `wasmi` feed against 2.0.2 names GHSA-qrr7-2g7f-r9rg for `wasmi` and `wasmi_ir`; a `tlsn` advisory is matched against the tag). Counter-checks, each reverted: `atty =0.2.14` added to `hyalos-os/profile/Cargo.toml` made `review-scope` fail (`atty 0.2.14 is a direct dependency of a hyalOS crate but has no row`) and `cargo deny` fail naming RUSTSEC-2021-0145 and RUSTSEC-2024-0375; the live `wasmi` feed with its range widened to `<= 2.0.3` made `cargo xtask advisories` fail naming GHSA-qrr7-2g7f-r9rg; a planted `tlsn-core` advisory failed naming it; the canary step, run locally, saw cargo-deny report RUSTSEC-2021-0003 |
| S-06.4 | `hyalos-os/review/pin-policy.md`, "What a move needs": the forgery corpus, the presentation differential, a fresh canary, a changelog read, in a pull request of its own |
| S-06.4.e1 | Dry run: a release with the forgery corpus, differential and canary passing and a changelog read naming no format change meets items 1 to 5; the policy's table, row 1, says "It may proceed" |
| S-06.4.e2 | Dry run: a release whose changelog read names a presentation format change; "When the presentation format changes" and the table's row 2 say it needs a new record decision, an ADR, first |
| 11.1 | Read against ADR-005's "Leaving the beta pin": a pull request of its own (item 1), the forgery corpus (2), the presentation differential (3), a fresh provider canary (4), a changelog read naming any format change (5), and a format change needs a new record decision first. Nothing added contradicts it; the additions say where each piece of evidence lives |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `context.md` §2, ADR-005 and S-06.1.e1: `wasmi =2.0.2` | `wasmi =2.0.3`, and the scope's rows and the e1 test name 2.0.3 | The developer's answer to this slice's block: GHSA-qrr7-2g7f-r9rg names 2.0.2, and #488 moved the pin with `hyalos.profile/2` (repository ADR-0134). The row still says the audits cover `0.31.0` and `0.36.0`-`0.38.0`, not this version | n (decided by the developer) |
| `design.md` §2: `upstream_audit` is a citation or `none found` with the date searched | The check also accepts `not searched (utility, <date>)`, only on an `excluded` row. Every value must carry a date | Writing `none found` for a search that was not made would be false. A covered row needs a real search; the check fails `not searched` on one | n |
| `design.md` §2: `review-scope` fails when "a listed version differs" | It also fails when a listed crate is locked at a second version with no row; `base64` 0.21.7 (under TLSNotary) has a row of its own | Otherwise a second copy of a listed crate could slip in beside the reviewed one | n |
| ADR-005 names no position for `ureq` or `bincode` | `ureq` has `use` (covered) and `internals` (excluded) rows; `bincode` a covered `use` row | `ureq` is the HTTPS client the audit and the monitor reach the public log with, over `rustls`: ADR-008's "our use of TLS". `bincode` decodes untrusted presentations | n |
| ADR-005: "Signature and hash crates" | Rows for `sha2`, `p256` (direct) and `k256` (through `tlsn-attestation`) | `k256` is not a direct dependency, but the attestation's signature rests on it | n |
| The previous run's draft | `kvm-ioctls` and `seccompiler` now cite X41 D-Sec's 2023 rust-vmm review at the commits its scope lists; `kvm-bindings` says `none found`, as that review does not list it; `p256` and `k256` give the audited commit | The draft cited X41 for both KVM crates and nothing for `seccompiler`; reading the report's scope section corrected both | n |
| Slice 11 Verify: `cargo deny --manifest-path hyalos-os/Cargo.toml check advisories --config hyalos-os/deny.toml` | Ran `cargo deny --manifest-path hyalos-os/Cargo.toml --config hyalos-os/deny.toml check advisories` | cargo-deny 0.20.2 takes `--config` before the subcommand only, and refuses it after `check` | n |
| Files: modify `crates/xtask/src/main.rs` | The checks are in `crates/xtask/src/scope.rs`, tests in `scope_tests.rs`; `main.rs` only dispatches | Slice 10's pattern (`hyalos.rs`) | n |
| `design.md`, **Modifier: integration**: RustSec is faked with "a fixture lockfile with a known advisory" | The daily job writes the canary crate (`smallvec =1.6.0`) at run time and requires cargo-deny to report RUSTSEC-2021-0003; no vulnerable lockfile is committed | A committed vulnerable lockfile would raise a standing Dependabot alert on the repository | n |
| Suggested path 3: a daily job covering RustSec and both GitHub feeds | It also runs `cargo deny` on `kernel-image/Cargo.lock` and `guest/Cargo.lock`, and `review-scope` | CI's `hyalos-os` job checks only the workspace lockfile, and the kernel image locks `wasmi` too | n |
| `ci.yml`'s `hyalos_os` change filter | Now also matches `crates/xtask/` | The `hyalos-os` job runs `cargo xtask review-scope`, so xtask is in its dependency closure (ADR-0124) | n |

## Decisions

- **Resumed after the block (developer, 2026-11-24).** The question was whether to move `wasmi` to 2.0.3, record an assessed exception, or both. The developer chose to **move `wasmi` to 2.0.3**. That is merged as #488 (`705cecb`): `wasmi =2.0.3` with `wasmi_core`, `wasmi_ir` and `wasmi_collections` 2.0.3 in the monitor, the audit and the kernel image, execution profile `hyalos.profile/2` (profile 1 still names 2.0.2), a `CHECKED_EQUAL` pair `("2.0.2", "2.0.3")` so profile 1 records still replay, and repository ADR-0134 (Accepted). Recorded on #467. The branch merged `origin/main` rather than rebasing, since it was already pushed.
- **`wasmi` 2.0.3's new upstream `unsafe`** (ADR-0134's Consequences) is in the `wasmi` `internals` row (covered): `wasmi::Error` holds its kind behind a `NonNull`, with `unsafe impl Send` and `Sync` and five `unsafe` blocks in `src/error.rs`. The 2.0.2 to 2.0.3 diff, read from the registry sources, also fixes the store-lane offset decoding in `wasmi_ir` (`simd` only) and regenerates the trap codes and `Break` from one macro; `wasmi_collections` has no source change.
- **Direct dependencies come from the lockfiles,** as the dependencies of each package with no `source` (the workspace members), dev and build dependencies included: 24 crates. None is in `guest/Cargo.lock`, and `kernel-image/Cargo.lock` adds only `wasmi`.
- **Components ADR-005 and ADR-008 split** (`wasmi`, the three TLSNotary crates, `sha2`, `p256`, `k256`, `rustls`, `tokio-rustls`, `ring`, `rcgen`) must have both a `use` and an `internals` row, and the check holds each `internals` row to the ADRs' position (covered for `wasmi`, excluded for the rest).
- **Trust anchors.** `webpki-roots` (1.0.12 under TLSNotary, 0.26.11 under `ureq`) and `rustls-webpki` 0.103.17 have no rows of their own; the `tlsn-core`, `rustls` and `ureq` `use` rows name them.
- **No TOML dependency.** The check reads the scope's restricted form and a lockfile's packages itself, and fails on any line it cannot read, so `crates/xtask` still depends only on `serde_json`.
- **The GitHub feed check matches every locked package,** not only listed ones, compares a git pin's tag without its `v`, and takes an advisory that names no package to affect every version of the repository's crate. Feeds are read with `gh api --paginate --slurp`.
- **No exception list for the GitHub feed.** A finding goes to the developer, as this slice's block did. RustSec exceptions stay in `hyalos-os/deny.toml` with an assessment under `hyalos-os/advisories/`.
- **Repository ADR-0135** (Proposed) records the scope, the two checks and the daily job, with its README row and the `hyalos-os/` and `crates/xtask` rows of `CLAUDE.md`'s map.

## Discoveries

- **The advisory check's first real run (2026-11-24)** found nothing live: RustSec (advisory database at `10db699`) passes on all three lockfiles, and the GitHub feeds hold GHSA-qrr7-2g7f-r9rg (`<= 2.0.2`), GHSA-g4v2-cjqp-rfmq (`>= 0.41.0 and <= 1.0.0`) and GHSA-75jp-vq8x-h4cq (`>= 0.15.0 and <= 0.31.0`) for `wasmi`, none of which includes 2.0.3, and no advisory for `tlsn`. RustSec still has no entry for GHSA-qrr7-2g7f-r9rg.
- **Yanked, not advised.** `aes` 0.9.0 (under TLSNotary's `mpz-core`) and `der` 0.8.0 (under `spki` under `tlsn-core`) are yanked on crates.io, with no yank message. Neither has a RustSec entry or a GitHub advisory (`gh api "/advisories?ecosystem=rust&affects=aes@0.9.0"` and `der@0.8.0` return none). cargo-deny warns on them; the deny policy's `yanked` level is the default warning. They move only with TLSNotary's pin.
- **CI's pull-request `cargo deny` reads only `hyalos-os/Cargo.lock`.** The kernel image's lockfile, which also locks `wasmi`, is checked only by the daily job. A later change could add it to the `hyalos-os` job.
- **For Slice 12:** the package should point the reviewer at `review/scope.toml` and `review/pin-policy.md`, say that a review cannot be described as covering TLSNotary's protocol and cryptography or the TLS crates' internals, and that `wasmi` 2.0.3's upstream `unsafe` is in scope through its `internals` row. The audit citations are a web search's result on the dates the rows give.
- **The "presentation differential"** ADR-005 requires before a pin move has no test in the repository; the pin policy says the first move adds it.
- **The daily job's first scheduled run** has not happened yet: it runs only from the default branch, so it first runs after this merges. A manual dispatch then is the way to see it pass.
