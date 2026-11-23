# Slice 1 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #427 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/427`, forked from `56cf401`, before any edit.

| Citation | Holds |
|---|---|
| `attest/probes/hidden_header.py`: three calls, verdicts, `SMALLEST`, `APPROVED`, `head` | Yes |
| `attest/src/lib.rs`: `CREDENTIAL_LENGTH`, `Trust::read`, `body`, `request`, `pinned_credential`, `link::read_private` | Yes |
| `attest/src/main.rs:139`: `credential` takes no path | Yes |
| `attest/src/bin/notary.rs:173`: `Sequences::save`, private to the notary | Yes |
| `attest/src/bin/standin.rs:81`: `serve` | Yes |
| `attest/tests/common/mod.rs`: `standin` | Yes |
| `out/live3.json` replays identically | Yes |

## Acceptance

Each row is a test of that name in `attest/tests/canary.rs`, unless it says otherwise.

| ID | Shown by |
|---|---|
| S-01.1, S-01.1.e1, e2 | `canary_s_01_1_e1_*`, `canary_s_01_1_e2_*` |
| S-01.2, S-01.2.e1 to e6 | `canary_s_01_2_e1_*` to `canary_s_01_2_e6_*` |
| S-01.2.e7 | `canary_s_01_2_e7_*`, compiled with `canary-test-hooks` |
| S-01.3, S-01.3.e1, e2, e3 | `canary_s_01_3_e1_*`, `canary_s_01_3_e2_*`, `canary_s_01_3_e3_*` |
| S-01.4, S-01.4.e1 to e5, e7 | `canary_s_01_4_e1_*` to `canary_s_01_4_e5_*`, `canary_s_01_4_e7_*` |
| S-01.4.e6 | `canary_s_01_4_e6_*`, compiled with `canary-test-hooks` |
| 1.1 | `canary_1_1_the_forged_request_is_the_probes`, a unit test in `attest/src/bin/canary.rs` |
| 1.2 | `canary_1_2_a_second_check_is_refused_by_the_lock` |
| 1.3 | `canary_1_3_a_default_build_ignores_the_rename_failure_setting` |

Verify results: every build finished. The attest suites passed (10, 2, 3, 41, 10, 22, 4 and 28), and the canary suite passed with `canary-test-hooks` (23). The monitor's tests passed (118 and 7). `out/live3.json` printed `replay identical`. The test-name `comm` printed nothing, and `read_private` appears in `canary.rs`. No test contacted a real provider, and the real provider path has not been run.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Criterion 1.1's test sits with the slice's tests | It is a unit test in `attest/src/bin/canary.rs` | The request builders are private to the `canary` binary, so the notary's library holds no credential-bearing code | n |

## Decisions

- When a non-pass result cannot be renamed but the state is removed, the check keeps the verdict's exit code: 1 for an accepted forgery and 2 otherwise. The design's exit 2 applies when nothing could be recorded or removed.
- A latched state is left untouched by a check, so its `checked` time stays the time of the latch.
- `canary reset` does not make the marker. The next check makes it, because a state then exists.
- A status line counts only in the exact form `HTTP/1.1 NNN`. Anything else is no status, which holds.
- `canary status` is left to Slice 4, whose criterion 4.1 owns it.

## Discoveries

- For Slice 2: `attest::canary::read` is the strict reader to share. It returns `Ok(None)` for no file and an error for anything malformed, and the notary must treat both as no pass.
- For Slice 2: a test rig that writes a state file must set mode 600, because the reader refuses open permissions.
- For the retro: the cold read's staged-result fixes held in code, with no contract change.
