# Slice 1 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #413 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/413`, forked from `a93e522`, before any edit.

| Citation | Holds |
|---|---|
| `kernel/src/main.rs`: `Answer`, `Transcript`, `execute`, `replay`, `declared_run`, `audit_record`, `audit_report`, `task_of`, `claim_of` | Yes |
| `kernel/src/ledger.rs`: `Audited`, `Claim`, `Auditor::audit` | Yes |
| Test rigs: `Rig` in `audit_tests.rs`, `World` in `forgery_tests.rs` | Yes |
| `out/live3.json` replays identically | Yes |

## Acceptance

Each row is a test of that name, in `kernel/src`, unless it says otherwise.

| ID | Shown by |
|---|---|
| S-01.1.e1, e2 | `record_s_01_1_e1_*`, `record_s_01_1_e2_*` |
| S-01.2.e1, e2 | `record_s_01_2_e1_*`, `record_s_01_2_e2_*` |
| S-01.3.e1, e2 | `record_s_01_3_e1_*`, `record_s_01_3_e2_*` |
| S-01.4.e1, e2, e3 | `record_s_01_4_e1_*`, `record_s_01_4_e2_*`, `record_s_01_4_e3_*` |
| S-01.5.e1, e2 | `record_s_01_5_e1_*`, `record_s_01_5_e2_*`, and the Verify replay of `out/live3.json` |
| S-01.7.e1 (legacy side) | `record_s_01_7_e1_*` |
| S-01.8.e1, e2, e3 | `record_s_01_8_e1_*`, `record_s_01_8_e2_*`, `record_s_01_8_e3_*` |
| S-01.9.e1, e2 | `record_s_01_9_e1_*`, `record_s_01_9_e2_*` |
| S-02.1.e1, e2, e3 | `record_s_02_1_e1_*`, `record_s_02_1_e2_*`, `record_s_02_1_e3_*` |
| S-02.2.e1, e2 | `record_s_02_2_e1_*`, `record_s_02_2_e2_*` |
| S-02.3.e1, e2 | `record_s_02_3_e1_*`, `record_s_02_3_e2_*` |
| S-05.9.e1, e2 | `record_s_05_9_e1_*`, `record_s_05_9_e2_*` |
| S-05.10.e1, e2 | `record_s_05_10_e1_*`, `record_s_05_10_e2_*` |
| 1.1 | `record_1_1_a_version_2_pass_says_the_budget_is_not_checked` |

Verify results: both release builds finished. The monitor's tests passed (78 and 7). The attest tests passed (9, 2, 10, 4 and 28), and this slice does not change that crate. `out/live3.json` printed `replay identical`. The test-name check and `git status --short ledger out` were both empty.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files: `main.rs`, `ledger.rs`, `audit_tests.rs`, `forgery_tests.rs`, `live_tests.rs` | `kernel/tests/audit_cli.rs` also changed | Its rig audits the committed version 1 record, which now needs `--legacy-v1` | n |

## Decisions

- Every command that takes a record reads it through one function, `read_transcript`, so an unknown version or a wrong version 2 `task` is refused the same way on audit, replay and `attested`.
- A version 2 `task` is checked when the record is read as well as on replay, so an audit refuses it without running the guest.
- `attested` refuses `--legacy-v1`, because it takes only the arguments it uses.

## Discoveries

- The monitor's tests need the attest programs built first. Without them, 24 of the 49 unit tests fail on an untouched branch.
- `cargo fmt --check` already reports differences in the spike on `main`. This slice did not reformat it, to keep the diff to the change.
- The kernel-wall `attacks` check in `context.md` §4 is not in this slice's Verify and was not run.
