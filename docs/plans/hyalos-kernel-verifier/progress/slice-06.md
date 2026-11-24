# Slice 6 progress

- **Status:** done
- **Issue:** #448
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `6a96b84`.

| Citation | Result |
|---|---|
| `release.yml` tag filter `v*`, static build and `--verify-tag` | Holds, lines 20, 195 and 207 |
| `build-release-binary.sh` builder digest and unpinned `musl-tools` | Holds |
| `release_gate.rs` detects tag jobs by `if: github.event_name == 'push'` | Holds |
| `docs/operations/releases.md:21` provenance command | Holds |
| `musl-tools` version in the pinned image | `1.2.3-1` |

## Acceptance

Workflow tests are in `crates/xtask/tests/release_gate.rs`, and binary tests in `hyalos-os/verify/tests/release.rs`.

| ID | How it was shown |
|---|---|
| S-05.1 (workflow side), e1, e2 | `s_05_1_e1_a_verifier_release_holds_the_binary_its_hash_and_provenance`, `s_05_1_e2_nothing_is_published_without_the_hyalos_os_evidence`, `s_05_1_the_manual_mode_takes_no_input_and_publishes_nothing`. A real run is Slice 7's |
| S-05.2 (documented side) | `s_05_2_the_adr_documents_both_provenance_checks`. A real check is Slice 7's |
| S-05.3.e1 (hash side) | `s_05_3_e1_a_binary_with_one_changed_byte_has_another_hash`. S-05.3.e2 is Slice 7's |
| S-05.4.e1, e2 | `.github/scripts/check-verifier-rebuild.sh` in CI's `hyalos-os` job. CI_RESULT |
| S-05.5.e1 | `s_05_5_e1_the_verifier_workflow_creates_no_tag`, and `nothing_but_the_operators_script_creates_a_tag` |
| S-05.5.e2 | `s_05_5_e2_a_tag_off_main_or_of_another_version_stops_first` |
| S-05.6.e1 | `s_05_6_e1_a_release_build_names_its_commit_and_what_it_reads`, and the rebuild check requires the real commit in `--version` |
| S-05.6.e2 | `s_05_6_e2_a_build_outside_a_checkout_says_the_commit_is_unknown`, `local_6_a_commit_that_is_not_one_is_never_named` |
| 6.1 | ADR-0131 and the rule in `CLAUDE.md` |
| 6.2 | `local_6_6_the_verifier_build_is_pinned_and_checked_in_ci` |
| 6.3 | `local_6_3_a_verifier_tag_never_starts_the_product_release` |
| 6.4 | `local_6_4_the_evidence_list_defaults_to_the_products` |
| 6.5 | The S-05.1 and S-05.5 tests, over `release-verifier.yml` |
| 6.6 | `local_6_6_the_verifier_build_is_pinned_and_checked_in_ci` |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Model the workflow on `release.yml`, whose provenance step is skipped on a private repository | The verifier's provenance step has no such condition | S-05.1 requires provenance on every release, so a repository that cannot attest publishes nothing | n |

## Decisions

- CI's `hyalos-os` job also runs when either verifier build script changes, so a change to the build cannot skip its own check.
- The three rebuilds run at once to stay inside the job's time budget.

## Discoveries

- For Slice 7: the operator must start `release-verifier.yml` by hand on `main` before S-05.1 to S-05.3 can be proven. No agent starts it.
