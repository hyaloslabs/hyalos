# Slice 7 progress

- **Status:** blocked
- **Issue:** #449
- **PR:** none (no PR is opened)
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23
- **Blocked on:** Has the operator run `release-verifier.yml` by hand on `main`, and what are the run's URL, the binary's SHA-256 and the `gh attestation verify` output? The stop condition is: "Do all other work first. Then, if the operator has not yet run the release workflow's manual mode on `main` and reported it, stop with status `blocked`, naming only the S-05.1, S-05.2 and S-05.3 real-run rows." No agent starts the workflow, creates a tag or creates an attestation.

## Preflight

Checked at base commit `2b3fe3a`.

| Citation | Result |
|---|---|
| The real record, its salt, presentations and notary key in `evidence/out/two-machines-canary/` | Holds |
| The chain in `evidence/ledger/` and the operator key in `evidence/out/two-machines-slice5/operator.pub` | Holds |
| The public log lists exactly this chain under the operator's key | Holds: the verifier passed against the live log |
| `hyalos-verify` command, `design.md` §2 | Holds |
| A manual run of `release-verifier.yml` | None exists |

## Acceptance

Tests are in `hyalos-os/verify/tests/real.rs`.

| ID | State | How it was shown |
|---|---|---|
| S-05.1 (real run) | **Blocked** | Needs the operator's manual run |
| S-05.2 (real run) | **Blocked** | Needs the operator's manual run |
| S-05.3 (real run) | **Blocked** | Needs the operator's manual run |
| S-06.1.e1 | Shown | `s_06_1_e1_a_real_record_passes_against_the_public_log`, and a `FROM scratch` container holding only the static verifier and its inputs, with an empty `PATH`: exit 0 |
| S-06.1.e2 | Shown | `s_06_1_e2_with_no_network_it_is_inconclusive`, and the same container with `--network none`: exit 2, no report |
| S-06.2.e1 | Shown | `s_06_2_e1_one_changed_byte_in_the_transcript_does_not_pass`, exit 1 |
| S-06.2.e2 | Shown | `s_06_2_e2_one_changed_byte_in_a_presentation_does_not_pass`, `s_06_2_e2_one_changed_byte_in_a_link_does_not_pass`, exit 1 |
| S-06.3.e1 | **Partial** | The rebuild and the run were done, the run in a clean container. The download, attestation and `git clone --branch` steps need a first release, so they have not been followed |
| S-06.3.e2, 7.3 | Shown | `s_06_3_e2_local_7_3_the_guide_lists_every_limit_the_report_can_print`, 9 lines |
| S-06.4, e1, e2 | Shown | `s_06_4_the_record_format_describes_version_4_the_profile_and_the_options`, `s_06_4_e1_the_exit_codes_in_the_record_format_are_the_verifiers`, `s_06_4_e2_the_size_limits_in_the_record_format_are_the_verifiers` |
| 7.1 | Shown | `local_7_1_the_guide_says_whom_not_to_trust` |
| 7.2 | Shown | The S-06.1 run used these inputs |
| 7.4 | Shown | `local_7_4_the_public_log_requirement_is_listed`. The S-06 tests ran with `HYALOS_REQUIRE_PUBLIC_LOG=1`, and none skipped |
| 7.5 | **Blocked** | Needs the operator's run. The local rebuild of `2b3fe3a` hashed to `06b255641c1c5a54aa4ae9ec1c313af2614f002e5cdc82eb1537624295068be7`, which is not a release |

All 363 hyalOS tests pass with `test-trust`.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-06.1.e2 runs in a container with no network | The test blocks `connect` in the child with a seccomp filter. The real container run was done once by hand | A test must not need Docker or privileges, and a build without `test-trust` cannot be pointed at a dead log | n |

## Decisions

- CI does not set `HYALOS_REQUIRE_PUBLIC_LOG`, so an outage of the public log cannot fail every pull request. The slice's Verify command sets it.

## Discoveries

- When this resumes: record the operator's run, finish S-06.3.e1 if a release exists or say that it does not, write the README Retro and set its Status to `done`.
