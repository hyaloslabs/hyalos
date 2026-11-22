# Slice 6 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #418 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/418`, forked from `2b68c97`, before any edit.

| Citation | Holds |
|---|---|
| `kernel/src/main.rs`: `Answer`, `Host::infer`, `replay`, `read_transcript`, `audit_report` | Yes |
| `spikes/task-kernel/ABI.md`, as the pattern for a format document | Yes |
| `design.md` §2, the Provider-proof entry row, and **Modifier: public-api** | Yes |

## Acceptance

Rows S-06.1 to S-06.4 are tests of that name in `kernel/src/forgery_tests.rs`.

| ID | Shown by |
|---|---|
| S-06.1.e1, e2 | `record_s_06_1_e*`. e2 replays the committed `out/live3.json` |
| S-06.2.e1, e2 | `record_s_06_2_e*` |
| S-06.3.e1, e2 | `record_s_06_3_e*`, across each malformed form in the design |
| S-06.4.e1, e2 | `record_s_06_4_e*` |
| S-06.5.e1, e2 | `grep` on `RECORD.md`: `2026-11-22`, `FlashProve`, `verifiable proof through its API` |
| 6.1 | `grep -ci restor RECORD.md`, in the notary state section |

Verify results: both release builds finished. The monitor's tests passed (118 and 7). `out/live3.json` printed `replay identical`. The test-name check was empty, and each `grep` printed 1. The attest crate is unchanged and its tests were not re-run for this slice. No real provider was contacted.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| A proof on a version 1 record is refused in `audit_record` | It is refused in `read_transcript` | Every command reads a record there, so `replay` and `attested` refuse it too, as the design's "any entry on a version 1 record" requires | n |
| Tests in `audit_tests.rs` | They are in `forgery_tests.rs`, and `audit_tests.rs` only passes the report's new argument | They need a real attested record | n |

## Decisions

- The proof count is printed only on a version 2 pass, because a version 1 record cannot hold a proof.
- The entries are inside the transcript, so the result link's commitment covers them.

## Discoveries

- None for a later slice. This is the pack's last slice, and the README's Retro holds the lessons.
