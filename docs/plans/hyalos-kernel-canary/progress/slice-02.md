# Slice 2 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #428 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/428`, forked from `9ba830f`, before any edit.

| Citation | Holds |
|---|---|
| `attest/src/lib.rs`: `TICKET`, `read_ticket`, `link::open`, `LinkFailure`, `attest::canary::read` | Yes |
| `attest/src/bin/notary.rs`: `Sequences::spend`, `session`, `sign` | Yes |
| `attest/src/main.rs`: `Word`, `verify` | Yes |
| `kernel/src/live.rs`: `refusal` | Yes |
| `attest/tests/authorise.rs`: `record_4_2_a_requester_extension_is_refused` | Yes |

## Acceptance

Each row is a test of that name in `attest/tests/notary.rs`, unless it says otherwise.

| ID | Shown by |
|---|---|
| S-02.1, S-02.1.e1, e3 to e8 | `canary_s_02_1_e1_*`, `canary_s_02_1_e3_*` to `canary_s_02_1_e8_*` |
| S-02.1.e2 | `canary_s_02_1_e2_*` in `kernel/src/live_tests.rs`, a real notary and the monitor |
| S-02.2, S-02.2.e1, e2 | `canary_s_02_2_e1_*`, `canary_s_02_2_e2_*` |
| S-02.3, S-02.3.e1, e2 | `canary_s_02_3_e1_*`, `canary_s_02_3_e2_*`, and the Verify `grep`, which printed 0 |
| 2.1 | `canary_2_1_a_canary_is_read_only_in_its_one_form`, a unit test in `attest/src/lib.rs` |
| 2.2 | `canary_2_2_an_older_tool_hears_release` |

Verify results: every build finished. The attest suites passed (12, 2, 3, 41, 10, 23, 4 and 40). The monitor's tests passed (120 and 7). `out/live3.json` printed `replay identical`. The test-name `comm` printed nothing. No test contacted a real provider, and the real provider path has not been run.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files to modify did not list `attest/src/bin/forging-notary.rs` or `attest/tests/notary.rs` | Both changed | S-02.1.e8 needs a pass that goes stale between open and signing, which real time cannot do reliably. The test-only notary shifts the session time its signing check judges, and the real `notary` cannot compile with that hook | n |

## Decisions

- S-02.1.e7 is shown at the tool: the second session hears `canary` and spends no number. The monitor's mapping of that word to a refused run is shown by S-02.1.e2 and `canary_the_word_is_the_canary_reason_logged_as_refused`.
- S-02.3.e1 uses the PRD's second option, removing read permission from a credential file beside the notary. It shows the notary does not need the file. The source search in S-02.3.e2 is the stronger evidence that it never opens one.
- `verify` reads the canary extension and reports it. It judges no freshness, which is the audit's job in Slice 3.

## Discoveries

- For Slice 3: `forging-notary` has no canary forgery yet. It needs one for a missing, doubled, malformed or stale signed canary.
- For Slice 3: the test rigs write a fresh pass through `common::canary_pass` in attest and `live_tests::canary_pass` in the monitor.
- The attest tool's own unit tests start a notary too, so their rig in `attest/src/main.rs` also writes a pass.
