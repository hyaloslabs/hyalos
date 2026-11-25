# Slice 12: The review package

| | |
|---|---|
| **Status** | done |
| **Issue** | #468 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-25 |

## Preflight

Checked on branch `issue/468`, forked from `origin/main` at `7a6e648` (Slice 11 and #488 merged). Symbols were found by name.

| Citation | Holds? |
|---|---|
| Stop condition: #449 closed | Holds: closed |
| Slices 1, 4, 10 and 11, this slice's dependencies, and every other slice | Hold: all eleven progress files on `origin/main` say `done` |
| `context.md` §3, Pattern: the stated limits at `hyalos-os/audit/src/report.rs:6`, `:79–100` | Holds: `HIDDEN_BYTES_LIMIT` at `:6`, the `not proven:` lines at `:79–104` |
| Pattern: guide structure under `docs/security/` | Holds: `threat-model.md`, `data-flows.md`, `reviewing-the-core.md` |
| Files: `hyalos-os/review/README.md` absent | Holds. `review/` holds `scope.toml` and `pin-policy.md` (Slice 11) |
| Files: `crates/xtask/src/main.rs` | Holds. It dispatches to modules of their own (`hyalos.rs`, `scope.rs`), the pattern followed here |
| Suggested path 4: `docs/plans/hyalos-kernel/coverage.md:45`, the "Independent security review" row | Holds at line 45 |
| `design.md` §2, `cargo xtask review-package`'s inputs and failures | Hold as written |
| `design.md`, **Modifier: security**, "Limits to state in the package" | Holds, but names only `RDSEED`. ADR-009 adds `RDRAND` and `RDPID`; the package states all three (Deviations) |
| `hyalos-os/RECORD.md`'s randomness limit | Holds: it names `RDSEED`, `RDRAND` and `RDPID` (Slice 4) |
| `design.md` §2, the seccomp list; ADR-0132's measured list | The package states ADR-0132's list, `runner.rs`'s `ALLOWED` (Slice 7's deviations) |
| `design.md` §2, the CPUID allow-list; `kvm.rs`'s `allowed_cpuid` | Hold, with Slice 3's `IA32_APIC_BASE` write |
| Slice 12's Verify: `cargo xtask review-scope` | Holds (Slice 11) |

No difference touches this slice's contracts.

## Acceptance

The package is `hyalos-os/review/README.md`. Its check is `cargo xtask review-package` (`crates/xtask/src/package.rs`), with tests in `crates/xtask/src/package_tests.rs`. CI's `lint` job runs the check on every pull request.

| ID | How it was shown |
|---|---|
| S-07.1 | The package holds `## Scope` (`scope.toml` explained, and our own code), `## Threat model` (assets, entry points, the design's threat table with a test for each), `## Interface` (`hyalos.abi/1`, the start state, the CPUID list, `hyalos.runner/1`, the seccomp list and Landlock, the host probe), `## Limits`, `## Commit under review`, `## Commands` and `## Findings`. Tests `core_s_07_1_the_repository_package_passes`, `core_s_07_1_e1_every_required_section_is_present`. `cargo xtask review-package` printed `review package ok: 7 sections, commit under review is an ancestor of HEAD, 31 code references` |
| S-07.1.e1 | Test `core_s_07_1_e1_every_required_section_is_present`; `core_s_07_1_a_missing_empty_or_repeated_section_fails` removes each of the seven in turn, empties one and repeats one, and each fails naming it. A heading inside a code block does not count |
| S-07.1.e2 | Test `core_s_07_1_e2_a_package_naming_no_commit_fails` (a short hash, no hash, a hash only in another section, and 40 hex digits inside a longer word each fail). `core_s_07_1_the_commit_must_be_one_ancestor_of_head`: a commit that is not an ancestor, one git cannot check, and two different commits each fail. `core_s_07_1_the_ancestor_check_asks_git` runs the real `git merge-base --is-ancestor` on `HEAD` (passes) and on an all-zero hash (fails closed) |
| S-07.2 | Dry run from a clean clone: see the Verify section below |
| S-07.2.e1 | Clean clone on a KVM host: build and every test pass (Verify table). A no-context dry run by a separate agent (Haiku) followed the package's commands alone; its report had not arrived when this file was committed, so the no-context half of S-07.2.e1 is not shown here and is left to the review (#491) |
| S-07.2.e2 | Run in a namespace where `/dev/kvm` cannot be opened: tests pass, each KVM test prints `skipped: <test> needs KVM` (54 in the workspace, 2 in `release_switches`), and `probe-host` exits 2 naming the cause (Verify table) |
| S-07.3 | `## Findings`: report privately through `SECURITY.md`, never in a public issue; each finding closed by a fixing commit or an accepting ADR, and nothing may claim the review until every one is. Test `core_s_07_3_findings_say_how_to_report_and_how_each_is_closed` |
| S-07.3.e1 | Dry run: a fixed finding is closed by "a commit on `main` that fixes it, with a test that fails before the fix. The finding cites the fixing commit" (`## Findings`, first bullet). The test asserts `cites the fixing commit` |
| S-07.3.e2 | Dry run: an accepted finding is closed by "an ADR under `docs/adr/` that states the limit ... The finding cites the ADR", and the limit is added to `## Limits` (second bullet). The test asserts `cites the ADR` |
| 12.1 | The package's first line after its title contains `not yet been independently reviewed`. Tests `core_12_1_the_package_says_it_has_not_been_reviewed` (missing words fail), `core_12_1_a_line_claiming_a_review_fails` (six claims, in any case, each fail; the required sentence beside other text does not). `core_12_1_the_package_states_every_limit_in_the_design` reads `design.md`'s "Limits to state in the package" and requires each limit it names in the package's `## Limits`, and `RDRAND`, `RDSEED` and `RDPID` (ADR-009). `core_12_1_no_line_claims_a_taken_over_kernel_lacks_randomness_everywhere` holds the slice's stop condition: the phrase appears only after `does not claim` |
| 12.2 | #491, "Engage an independent reviewer for hyalOS", filed first, citing ADR-006, #412 and #468, with done-when items for choosing a reviewer, agreeing the scope, private reporting and closing each finding by commit or ADR. `docs/plans/hyalos-kernel/coverage.md:45` now points to the package and #491. The package names #491 in its opening and in `## Findings` |

## Verify

All from a clean clone of this branch at `149b62e` (the package and its check), made with `git clone --quiet . "$d/repo"` as the Verify block does, on the development host (Intel, WSL2, nested KVM, Landlock ABI 3), 2026-11-25.

| Command | Result |
|---|---|
| `cargo xtask review-scope` | exit 0: `hyalOS review scope: 50 rows agree with 3 lockfiles` |
| `cargo xtask review-package` | exit 0: `review package ok: 7 sections, commit under review is an ancestor of HEAD, 31 code references` |
| Verify's third command, exactly, `HYALOS_REQUIRE_KVM=1` | exit 101: `canary_s_02_1_e8_a_pass_stale_at_signing_gets_no_attestation` panicked with `forging-notary is not built`, and cargo stopped at that test binary. The known need for CI's two `attest` builds (Deviations) |
| The package's **Common setup** (targets, the two `attest` builds) | exit 0, exit 0, exit 0 |
| The package's KVM workspace test, `HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1` | exit 0: 462 passed, 0 failed, none skipped (with both variables set a skip would fail) |
| `release_switches`, both variables set | exit 0: 5 passed |
| `task-kernel fuzz … 100000` | exit 0: `100000 random requests: 7075 answered, 92925 refused, no crash` |
| `task-kernel probe-host` | exit 1: `RDTSC blocked`, `RDTSCP blocked`, `RDRAND blocked`, `RDSEED reachable`, `RDPID blocked` |
| `cargo xtask lines`, `hyalos-unsafe` | exit 0: hyalOS 9,042 of 12,000; `unsafe only in the 11 listed files` |
| `cargo fmt … --check`, the package's `clippy … -D warnings` | exit 0, exit 0 |
| `cargo deny --manifest-path hyalos-os/Cargo.toml --config hyalos-os/deny.toml check` | exit 0: `advisories ok, bans ok, licenses ok, sources ok` |
| The package's no-KVM set, in the namespace described under Deviations | Workspace tests exit 0: 462 passed, 0 failed, 54 `skipped:` lines, each `needs KVM` or `needs KVM and Landlock`. `release_switches` 5 passed, 2 `skipped: … needs KVM and Landlock`. `probe-host` exit 2: `could not test: no permission to open /dev/kvm` |

Root checks on the worktree: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo xtask layout|lines|hyalos-unsafe|docs|review-scope|review-package`, `cargo deny check` all pass. `cargo test -p xtask`: 62 passed, the 12 new ones included. The root `cargo test --workspace --locked` was not run locally (CI's `test` job is the check for it).

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `design.md`, **Modifier: security**: the limits name `RDSEED` alone | The package states `RDRAND`, `RDSEED` and `RDPID` as host-dependent, with a table of the hosts measured (the development host and three CI runner types, by run) | ADR-009 amends ADR-002, and Slices 3 and 4 asked this slice to state all three. The design is not edited (README rule 6); the 12.1 test checks every design limit and the three instructions | n |
| Verify's third command passes from a clean clone | Exit 101 from a clean clone: `forging-notary is not built`. With the package's **Common setup** first, the same command passes | As Slices 1 to 11 found: `attest/tests/notary.rs` runs the built `attest` programs. The package's command set builds them first, as CI does | n |
| Files: create `README.md`, modify `crates/xtask/src/main.rs` and `coverage.md` | Also `crates/xtask/src/package.rs` and `package_tests.rs` (new), `.github/workflows/ci.yml` and `CLAUDE.md` | Slices 10 and 11's pattern, a module of its own. The CI step runs the check, and the `lint` job's checkout now fetches full history (`fetch-depth: 0`) so the ancestor check can see the commit under review; on a shallow clone it fails closed. `CLAUDE.md`'s map names the package and the check | n |
| `design.md` §2: `review-package` fails on a missing section, a commit that is not an ancestor, the missing sentence, or another line claiming review | It also fails on an empty or repeated section, a commit section naming no full hash or two different ones, a commit git cannot check, and a `` `path.rs::name` `` reference that does not resolve (the same rule as `cargo xtask docs`) | Each fails closed on a form the design's rules would let through. The references keep the package's 31 citations of code and tests from going stale | n |
| Contract, Commit under review: "the package's own pull request adds only the package and its check" | It also changes the plan's notes (this file, the pack README, `coverage.md`) and `CLAUDE.md`'s map | They are documentation the pack and repository rules require in the same pull request. No file under `hyalos-os/` other than the package changes, which the package's `git diff --stat` command shows | n |
| `design.md` §6: S-07.2.e2 on "a host without `/dev/kvm`" | Run on the development host inside a user and mount namespace where `/dev/kvm` is a bind-mounted file with mode `000`, and no capabilities, so it cannot be opened | No host without KVM was at hand. The tests' gate opens `/dev/kvm` read-write, exactly what the namespace refuses. The tests ran as the namespace's root without capabilities, not as an ordinary user | n |

## Decisions

- **Core dumps are a stated limit, not fixed here.** Slice 7 suggested setting `RLIMIT_CORE` to 0 in the runner. This slice's contract says its pull request adds only the package and its check, so the package states the limit (ADR-0132's Consequences) and #492 tracks the fix.
- **The commit under review** is `7a6e6481c0f07939c295f0baa87412d24f16af04`, the branch's merge-base with `main`. If `main` is merged into the branch before it lands, the merge-base moves and the package must name the new one; the check still passes with the old one, since it stays an ancestor.
- **What counts as a claim.** A fixed list of phrases (`has been reviewed`, `review is complete`, `reviewed by` and others), read in lower case with the required sentence taken out first. It is textual, so it errs towards failing: the package's own description of the check had to be reworded once.
- **The ancestor check in tests.** The real package is checked with the ancestor taken as given, since CI's `test` job checks out one commit; `cargo xtask review-package` in the `lint` job, which fetches full history, makes the real check, and a unit test runs real `git` on `HEAD`.
- **#491's labels** are `security` and `needs-decision`, since choosing a reviewer is the developer's decision (ADR-006). #492 is `security`.
- **The Retro** is in the pack README, with Status `done`, as rule 8 and pack mode require for the last slice.

## Discoveries

- **CI's runners answer the README's open questions in part.** `probe-host` in CI's `hyalos-os` job measured: Xeon 6975P-C (`kvm_intel`) all blocked (runs `45547863519`, `45551214113`); Xeon Platinum 8573C `RDSEED` reachable (run `45556140227`); AMD EPYC 7763 `RDRAND`, `RDSEED` and `RDPID` reachable (run `45552923599`). So `RDSEED` is reachable on a non-nested Intel CI host too, and AMD leaves all three reachable, as ADR-009 predicted. No bare-metal host has been measured.
- **Landlock's ABI on CI is not recorded.** No test or step prints the ABI the runner's kernel enforces, so whether `IOCTL_DEV` (ABI 5) and the scopes (ABI 6) are ever applied in CI is unknown. A one-line step printing it would settle it.
- **For the review (#491):** the most direct paths for a reviewer are in the package's threat model, and the open issues it lists (#490, #484, #487, #492) are known before they start.
- **Skip messages are captured.** Without `--nocapture`, `cargo test` hides the `skipped:` lines of passing tests, so the package's no-KVM set keeps the output and greps it. Some skips interleave with other output under `--nocapture`, so a count by grep is approximate.
- `verify/tests/real.rs` skips when it cannot reach the public log; it reads only, and the KVM run here could not show whether it skipped, since its output was captured.
