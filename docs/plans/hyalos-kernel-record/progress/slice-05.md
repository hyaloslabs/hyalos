# Slice 5 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #417 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/417`, forked from `1ccbb61`, before any edit.

| Citation | Holds |
|---|---|
| `kernel/src/main.rs`: `check_answer`, `witnessed`, `audit_record`, `audit_report`, `claim_of` | Yes |
| `kernel/src/forgery_tests.rs`: `World` | Yes |
| `attest/src/bin/notary.rs`: `sign` | Yes |
| `design.md` §2, the Ticket rows of **Audit** and the report wording, and §6, **Forged tickets in tests** | Yes |

## Acceptance

Each row is a test of that name in `kernel/src/forgery_tests.rs`.

| ID | Shown by |
|---|---|
| S-01.7.e2 | `record_s_01_7_e2_a_version_2_pass_opens_with_audit_passed` |
| S-05.1.e1, e2, e3 | `record_s_05_1_e*`, through `forging-notary` |
| S-05.2.e1, e2, e3 | `record_s_05_2_e*` |
| S-05.3.e1, e2 | `record_s_05_3_e*` |
| S-05.4.e1, e2, e3 | `record_s_05_4_e*`, through `forging-notary` |
| S-05.5.e1, e2 | `record_s_05_5_e*`, through `forging-notary` |
| S-05.6.e1, e2 | `record_s_05_6_e*` |
| S-05.7.e1 to e5, e8 | `record_s_05_7_e*` |
| 5.1 | `grep -c HYALOS_FORGE_TICKET` on the release `notary` prints 0 |
| 5.2 | `notary` with `forge-ticket` fails to compile: `cannot find function forged_ticket in the crate root` |

Verify results: every build finished except the `notary` build that must fail. The attest tests passed (10, 2, 41, 10, 4 and 28). The monitor's tests passed (110 and 7), and the 110 passed six runs in a row after the flaky test below was fixed. `out/live3.json` printed `replay identical`. The test-name check was empty. No real provider was contacted.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `forging-notary` includes `notary.rs`'s source | It loads it as a module with `#[path]` | `notary.rs` opens with inner doc comments, which `include!` does not accept. The hook is still defined only in `forging-notary.rs` | n |
| Files omit `attest/src/main.rs` | `ask` gains a test-only `HYALOS_TEST_KEEP_PRESENTATION` | `ask`'s own `verify` refuses a doubled or short ticket before writing the file, so the audit could not otherwise be shown that file | n |
| Files omit `kernel/src/live_tests.rs` | One test script now reads its input | Slice 4's script could exit before the monitor wrote the question, so the broken pipe intermittently became `ModelUnavailable`. The real `ask` reads its input first | n |
| Test `record_1_1_*` | Replaced by `record_s_01_7_e2_*` | It pinned the interim first line, which this slice removes | n |

## Decisions

- A pass opens with `audit passed` only for a record that is not version 1 and carries an attempt, so a report without one cannot claim a budget.
- S-05.5.e2 uses `forging-notary` to set its round and prior total. The report reads only the signed fields.
- S-05.7.e8 derives the run hash the intent would have under a second key, asks under it, and places that attestation in this chain.

## Discoveries

- The notary never sees an operator log key, so S-05.7.e4 is a further new run against the same budget.
