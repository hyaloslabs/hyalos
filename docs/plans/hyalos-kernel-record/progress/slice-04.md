# Slice 4 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #416 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/416`, forked from `fa1cf2c`, before any edit.

| Citation | Holds |
|---|---|
| `attest/src/bin/notary.rs`: `spend`, `Notary::session`, `Notary::sign` | Yes |
| `attest/src/lib.rs`: `read_run_sequence`, `link::open` | Yes |
| `kernel/src/live.rs`: `NOTARY_SETTINGS`, `refusal` | Yes |
| TLSNotary at `40a0695`: the builder appends extensions, and the default validator refuses a requester's | Yes |
| `design.md` §2, **Link and refusals**, **Ticket spending**, **Ticket extension** | Yes |

## Acceptance

| ID | Shown by |
|---|---|
| S-01.6.e1, e2 | `record_s_01_6_e1_*`, `record_s_01_6_e2_*` in `kernel/src/forgery_tests.rs` (see Deviations) |
| S-03.5.e1, e2 | `record_s_03_5_e1_*`, `record_s_03_5_e2_*` in `attest/tests/authorise.rs` |
| S-04.1 to S-04.6, S-04.9, every row | `record_s_04_{1,2,3,4,5,6,9}_e*` in `attest/tests/authorise.rs` |
| S-04.7.e2 | `record_s_04_7_e2_a_run_from_old_state_is_refused` |
| S-04.8.e1, e2, e3 | `record_s_04_8_e*` in `kernel/src/live_tests.rs` |
| 4.1 | `record_4_1_both_extensions_are_read`, a real MPC-TLS session against the stand-in |
| 4.2 | `record_4_2_a_requester_extension_is_refused` |

Verify results: both release builds finished. The attest tests passed (10, 2, 41, 10, 4 and 28). The first full run failed one test in the `notary` file because the rig passed an empty authorisation. After that test-only fix the `notary` file was re-run alone and passed 28. The monitor's tests passed (89 and 7). The test-name check was empty. No real provider was contacted.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-01.6.e2 uses a real version 1 record with no ticket | The record's presentation comes from this notary, and a test wrapper around `verify` reports its `ticket` as `null` | This notary always signs a ticket, so a truly ticketless presentation needs the previous release's notary. S-01.6.e1 runs the real `verify`, so the refusal itself is not shimmed | n |
| Files list omits `attest/tests/binding.rs`, `cost.rs`, `notary.rs` | They changed | `tool` and `link::open` take the authorisation | n |

## Decisions

- `ask` has a test-only `HYALOS_TEST_REQUEST_EXTENSION`, like the existing test hooks. It only makes the operator's side ask for more, which a forger can do anyway, and the notary refuses it.
- A run that already holds a ticket skips the round and budget checks, so it can finish after a newer round or at the budget.
- A notary reads only the named authorisation for a known run, and all of them for a new run, because finding the newest round needs them all.

## Discoveries

- S-04.3.e2 cannot differ from S-04.3.e1 at the notary, which never sees an operator log key. The test shows a further new run is counted against the same budget.
