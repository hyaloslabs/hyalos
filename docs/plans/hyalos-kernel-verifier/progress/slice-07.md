# Slice 7 progress

- **Status:** done
- **Issue:** #449
- **PR:** this PR
- **Executor model:** Claude Opus 5.5
- **Date:** 2026-11-23

## Preflight

Checked at base commit `2b3fe3a`.

| Citation | Result |
|---|---|
| The real record, its salt, presentations and notary key in `evidence/out/two-machines-canary/` | Holds |
| The chain in `evidence/ledger/` and the operator key in `evidence/out/two-machines-slice5/operator.pub` | Holds |
| The public log lists exactly this chain under the operator's key | Holds: the verifier passed against the live log |
| `hyalos-verify` command, `design.md` §2 | Holds |
| A manual run of `release-verifier.yml` | The operator ran it: [run 45308242548](https://github.com/hyaloslabs/hyalos/actions/runs/45308242548), on `main` at `2b3fe3a` |

## Acceptance

Tests are in `hyalos-os/verify/tests/real.rs`.

| ID | State | How it was shown |
|---|---|---|
| S-05.1 (real run) | Shown | The run produced the binary, `SHA256SUMS` (`sha256sum -c` passes) and provenance, and its publish job was skipped. See Deviations for failed evidence |
| S-05.2 (real run) | Shown | The `refs/heads/main` form exits 0 on the run's binary. It exits 1 on a binary built locally from another commit (`2c74b566...`), which has no attestation |
| S-05.3 (real run) | Shown | A copy with one changed byte (`0bebda63...`) has another hash, and the provenance check exits 1 |
| S-06.1.e1 | Shown | `s_06_1_e1_a_real_record_passes_against_the_public_log`, and a `FROM scratch` container holding only the static verifier and its inputs, with an empty `PATH`: exit 0 |
| S-06.1.e2 | Shown | `s_06_1_e2_with_no_network_it_is_inconclusive`, and the same container with `--network none`: exit 2, no report |
| S-06.2.e1 | Shown | `s_06_2_e1_one_changed_byte_in_the_transcript_does_not_pass`, exit 1 |
| S-06.2.e2 | Shown | `s_06_2_e2_one_changed_byte_in_a_presentation_does_not_pass`, `s_06_2_e2_one_changed_byte_in_a_link_does_not_pass`, exit 1 |
| S-06.3.e1 | Shown, with no release yet | As the plan allows, the download step was exercised with the documented rebuild: a fresh clone at `2b3fe3a` rebuilt byte-identical to the run's binary. The attestation check used its `refs/heads/main` form, and the run's binary passed the real record. The `verifier-v*` download and `--branch <tag>` wording await the first release |
| S-06.3.e2, 7.3 | Shown | `s_06_3_e2_local_7_3_the_guide_lists_every_limit_the_report_can_print`, 9 lines |
| S-06.4, e1, e2 | Shown | `s_06_4_the_record_format_describes_version_4_the_profile_and_the_options`, `s_06_4_e1_the_exit_codes_in_the_record_format_are_the_verifiers`, `s_06_4_e2_the_size_limits_in_the_record_format_are_the_verifiers` |
| 7.1 | Shown | `local_7_1_the_guide_says_whom_not_to_trust` |
| 7.2 | Shown | The S-06.1 run used these inputs |
| 7.4 | Shown | `local_7_4_the_public_log_requirement_is_listed`. The S-06 tests ran with `HYALOS_REQUIRE_PUBLIC_LOG=1`, and none skipped |
| 7.5 | Shown | Below |

All 363 hyalOS tests pass with `test-trust`.

## The operator's run

- **Run:** <https://github.com/hyaloslabs/hyalos/actions/runs/45308242548>
- **Binary SHA-256:** `06b255641c1c5a54aa4ae9ec1c313af2614f002e5cdc82eb1537624295068be7`
- **`gh attestation verify`:** exit 0, with the signer workflow and `--source-ref refs/heads/main`. The certificate reads:

```json
{"buildSignerURI":"https://github.com/hyaloslabs/hyalos/.github/workflows/release-verifier.yml@refs/heads/main","issuer":"https://token.actions.githubusercontent.com","runInvocationURI":"https://github.com/hyaloslabs/hyalos/actions/runs/45308242548/attempts/1","sourceRepositoryDigest":"2b3fe3aa055f2212e20e34e1f6207937cdcab5c6","sourceRepositoryRef":"refs/heads/main","sourceRepositoryURI":"https://github.com/hyaloslabs/hyalos","subjectAlternativeName":"https://github.com/hyaloslabs/hyalos/.github/workflows/release-verifier.yml@refs/heads/main"}
```

## The first release

Added on 2026-11-24, after this slice merged. It completes S-06.3.e1's download and tag steps, which needed a release.

- **Tag:** the operator pushed `verifier-v0.1.0` at `eecae3b`, after #451 was settled. The release run, [45391056614](https://github.com/hyaloslabs/hyalos/actions/runs/45391056614), passed its evidence, build and publish jobs.
- **Step 1:** `gh release download` fetched `hyalos-verify-x86_64-linux` and `SHA256SUMS`, and `sha256sum -c` passed. The binary's SHA-256 is `65a7ba1911df9357150e56ca22c584299e22b300f0828f460db08c3ffa2fef43`.
- **Step 2:** `gh attestation verify` with `--source-ref refs/tags/verifier-v0.1.0` exits 0. Its certificate names that tag, commit `eecae3b` and run 45391056614. A copy with one changed byte exits 1, and so does the same copy checked against `refs/tags/verifier-v9.9.9`.
- **Step 3:** `git clone --branch verifier-v0.1.0` followed by `.github/scripts/build-verifier.sh dist` rebuilt a byte-identical binary.
- **Step 5:** the downloaded binary, run with an empty environment, verified the committed real record against the live public log. It printed `audit passed` with exit 0, and the 8 `not proven` lines the guide lists for a version 4 record.

Step 4 has nothing to run. The run used the keys committed beside the record.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-06.1.e2 runs in a container with no network | The test blocks `connect` in the child with a seccomp filter. The real container run was done once by hand | A test must not need Docker or privileges, and a build without `test-trust` cannot be pointed at a dead log | n |
| S-05.1: a run with failed evidence publishes nothing | No real run with failed evidence was made. `release-evidence.sh <commit> hyalos-os` exits 1 on `3b41ad9` and `02f66d1`, which lack that evidence, and `s_05_1_e2` proves publishing needs the evidence job | Forcing a real failed run would need another operator run for no further proof of the gate | n |

## Decisions

- CI does not set `HYALOS_REQUIRE_PUBLIC_LOG`, so an outage of the public log cannot fail every pull request. The slice's Verify command sets it.

## Discoveries

- The first real `verifier-v*` tag was the operator's, after #451 was settled. See **The first release**.
