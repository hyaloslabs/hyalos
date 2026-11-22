# Slice 6 — The notary reports what an attestation costs

| | |
|---|---|
| **Status** | done |
| **Issue** | #391 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-22 |

## Preflight

Checked at `83dcc0a`.

| Citation | Holds |
|---|---|
| `design.md` §2, The session line: four fields | holds |
| `design.md` §2, The usage file and Notary: cost report | holds, neither existed in code |
| `attest/src/bin/notary.rs`, the serving notary and its session lines | holds |
| The stand-in provider and the test rig in `attest/tests/common/mod.rs` | holds |
| Slice 3's real-provider time and round trip, in `progress/slice-03.md` | holds: 7.42 s, 2.9 ms |

## Acceptance

From `spikes/task-kernel`: the build ended `Finished`, and every test binary of the attestation crate printed `test result: ok` with `0 failed`, 53 tests in all. No test or command in this slice contacted the real provider.

| ID | Shown by |
|---|---|
| S-05.1 | `s_05_1_e1_s_05_2_t_6_1_forty_sessions_give_the_notarys_cost_and_the_operators_wall_time` |
| S-05.1.e1 | The same test, with and without a wall-time file |
| S-05.1.e2 | `s_05_1_e2_five_sessions_say_how_many_and_that_it_is_not_yet_a_measurement` |
| S-05.2 | The same 40-session test times each call and gives the report the file |
| 6.1 | The entry marked as a deviation under `design.md`, Modifier: performance, and the table below |

Also: `s_05_1_a_restart_starts_a_new_life_for_the_figures`, `s_05_1_a_report_it_cannot_read_or_reconcile_is_a_refusal`.

The report from the 40 sessions:

```text
sessions: 40 attested, 0 refused, in one life of the serving process
notary CPU: 1.211 s per attestation
notary peak memory: 548736 KB, one figure for the process
link bytes per attestation: 1114950 sent by the notary, 104361350 received
operator's wall time, without the real provider: 0.595 s per call over 40 calls
operator's wall time, with the real provider: 7.420 s per call over 1 call(s)
```

| Wall time | Figure | What it is |
|---|---|---|
| Without the real provider | 0.595 s per call | This slice's 40 calls, stand-in on loopback |
| With the real provider | 7.42 s, at a 2.9 ms round trip | Slice 3's one call across two machines, through an SSH tunnel |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-05.1 reads as peak memory per attestation | One peak for the process | `The usage file` already says so, and criterion 6.1 records it for the developer | n, accepted by the developer on 2026-11-22, recorded in `design.md` |

## Decisions

- The notary keeps the usage file as `usage` beside `sessions.log`. It is a labelled text file, rewritten whole after each session and reset when the process starts.
- The command is `notary report --dir <directory> [--wall <file>] [--real-wall <file>]`. Each wall-time file holds one call's seconds per line.
- Link bytes count only connections that proved the enrolled key, so a stranger cannot inflate the cost per attestation.
- Below 40 attested sessions the report gives the count and says it is not yet a measurement, and prints no figures.
- The report refuses when the newest session lines do not match the usage file's counts, or when a `--wall` file does not hold one time per attested session.

## Discoveries

- **For a public notary:** the operator's side received about 1.1 MB and the notary about 104 MB per attestation, so the operator-to-notary direction dominates the link's cost.
- **For the developer:** a later real run measured 4.35 s for one call on the same two machines. M2 holds for both calls, and two calls are not a distribution.
- **Still open from Slice 2:** a stranger can still grow `sessions.log`. The report now reads it holding only the newest lines, and the file itself has no bound.
