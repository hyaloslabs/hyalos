# Slice 2 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #414 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/414`, forked from `014012f`, before any edit. Line numbers moved with Slice 1, and each symbol below still does what `context.md` and the plan say.

| Citation | Holds |
|---|---|
| `kernel/src/main.rs`: `replay`, `audit_inputs`, `audit_record` | Yes |
| `kernel/src/forgery_tests.rs`: `World` | Yes |
| `design.md` §2, the Replay and Operator key given rows and the backend paragraph | Yes |

## Acceptance

Each row is a test of that name in `kernel/src/forgery_tests.rs`.

| ID | Shown by |
|---|---|
| S-05.8.e1 | `record_s_05_8_e1_a_version_2_audit_with_no_guest_is_refused` |
| S-05.8.e2 | `record_s_05_8_e2_a_guest_with_one_byte_changed_is_refused` |
| S-05.8.e3 | `record_s_05_8_e3_the_records_guest_and_a_record_that_replays_are_not_refused` |
| S-05.7.e6 | `record_s_05_7_e6_extra_sessions_written_into_the_record_diverge` |
| S-05.7.e7 | `record_s_05_7_e7_a_reworded_question_diverges` |
| 2.1 | `record_2_1_a_version_1_record_needs_no_guest`, which also shows the first line the dry run names |

Verify results: both release builds finished. The monitor's tests passed (84 and 7). `out/live3.json` printed `replay identical` on the in-process backend. The test-name check and `git status --short ledger out` were both empty. The attest crate is unchanged and its tests were not re-run for this slice.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files include `audit_tests.rs` | It did not need a change | Replay is in `audit_record`, which `World` drives. `Auditor::audit` is unchanged | n |
| Dry run for 2.1 by hand | Shown by a test | The test audits a real version 1 record with no guest and asserts the same first line | n |

## Decisions

- The guest's presence and hash are checked first, and replay runs after the attestations. A forgery therefore still meets the check that names it, and nothing reaches the report without replay.
- The replay branch is taken for every record that is not version 1, so a version added later is replayed by default.
- `attested` refuses `--guest`, because it takes only the arguments it uses.

## Discoveries

- `World` now uses a real generated guest, so its honest records replay. Slices 4 and 5 can use `World::asking` for runs of a chosen length.
