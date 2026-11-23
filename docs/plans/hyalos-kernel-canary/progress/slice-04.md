# Slice 4 progress

## Header

| | |
|---|---|
| **Status** | done |
| **Issue** | #430 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked on `issue/430`, forked from `c0a3043`, before any edit.

| Citation | Holds |
|---|---|
| `spikes/task-kernel/RECORD.md`: its sections and style | Yes |
| `attest/probes/two_machines.sh`: the built-programs check and the `authorize` step | Yes |
| `attest/src/bin/canary.rs`: `check`, `reset`, and no `status` yet | Yes |
| `attest::canary::fresh_pass` | Yes |

## Acceptance

Each row is a test of that name in `attest/tests/canary.rs`.

| ID | Shown by |
|---|---|
| S-04.1, S-04.1.e1, e2 | `canary_s_04_1_e1_*`, `canary_s_04_1_e2_*`, and the Verify `grep`s (2, 4 and 2) |
| S-04.2, S-04.2.e1, e2 | `canary_s_04_2_e1_*`, `canary_s_04_2_e2_*`, which run the script with stub programs |
| 4.1 | `canary_4_1_status_is_fresh_only_on_a_fresh_pass`, `canary_4_1_status_names_no_credential` |

Verify results: every build finished. The attest suites passed (12, 2, 3, 41, 10, 29, 4 and 40). After the review moved the script's `mkdir` below the gate, which touched only the script and its test, the canary suite was run three more times (29) and once with `canary-test-hooks` (30). The monitor's tests passed (137 and 7). `out/live3.json` printed `replay identical`. The test-name `comm` printed nothing, and `bash -n` accepted the script. The script's tests ran with stubs, and this host has `/usr/bin/time`, so they did not skip. No test contacted a real host, the provider, the public log or an issue. **The real two-machine run has not been made. It is the operator's, and it needs the customer to run `canary check` on the notary machine first.**

## Deviations

None.

## Decisions

- The script copies `canary` beside the notary and runs only `canary status` there. It never reads or names the customer's credential.
- `RECORD.md`'s example runs the check every six hours, so one failed check does not let the 24-hour window lapse.

## Discoveries

- For the retro: every slice's Verify ran locally because CI does not build this spike.
