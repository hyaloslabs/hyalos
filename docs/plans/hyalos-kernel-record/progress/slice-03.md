# Slice 3 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #415 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/415`, forked from `a9451e7`, before any edit.

| Citation | Holds |
|---|---|
| `attest/src/bin/notary.rs`: `Sequences`, `load`, `save`, `spend`, the serving lock, `main`'s dispatch | Yes |
| `attest/src/lib.rs`: `binding`, `link::read_private`, `link::keep_private` | Yes |
| `design.md` §2, **Authorisation** and the Sequences file row | Yes |

## Acceptance

Each row is a test of that name in `attest/tests/authorise.rs`.

| ID | Shown by |
|---|---|
| S-03.1.e1, e2 | `record_s_03_1_e1_*`, `record_s_03_1_e2_*` |
| S-03.2.e1, e2, e3 | `record_s_03_2_e1_*`, `record_s_03_2_e2_*`, `record_s_03_2_e3_*` |
| S-03.3.e1, e2 | `record_s_03_3_e1_*`, `record_s_03_3_e2_*` |
| S-03.4.e1, e2 | `record_s_03_4_e1_*`, `record_s_03_4_e2_*` |
| S-03.6.e1, e2 | `record_s_03_6_e1_*`, `record_s_03_6_e2_*` |
| S-03.7.e1, e2, and while serving | `record_s_03_7_e1_*`, `record_s_03_7_e2_*`, `record_s_03_7_tickets_reads_a_directory_the_notary_is_serving` |
| S-04.7.e1, e3 | `record_s_04_7_e1_*`, `record_s_04_7_e3_*` |
| S-04.10.e1, e2, e3 | `record_s_04_10_e1_*`, `record_s_04_10_e2_*`, `record_s_04_10_e3_*` |
| 3.1 | `record_3_1_two_authorisations_at_once_take_different_rounds` |

Verify results: the release build finished. The attest tests passed (9, 2, 20, 10, 4 and 28). The monitor's tests, run against the new notary, passed (84 and 7). The test-name check was empty.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Files include `attest/src/lib.rs` | It did not need a change | Only the notary reads authorisations, and the private-file helpers already exist | n |

## Decisions

- A starting notary reads its state under `authorisations.lock`, so no authorisation can appear while a version 1 file is being moved.
- A missing sequences file with authorisations present also refuses to start, because starting empty would hand spent tickets back.
- `notary tickets` refuses when a run holds a ticket of an authorisation that is not there, or when more tickets are spent than the budget gave.
- The tests give `authorize` a signing key file with no read permission and invalid contents, so a command that read it could not pass.

## Discoveries

- Until Slice 4, a new run is still served and is written with `- -`. Slice 4 must refuse any session for such a run with `authorisation`.
