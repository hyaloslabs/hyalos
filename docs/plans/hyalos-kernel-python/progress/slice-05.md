# Slice 5 — Hostile inputs, measurements and the findings

| | |
|---|---|
| **Status** | done |
| **Issue** | #498 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-25 |

## Preflight

Checked against the branch's tree, forked from `main` at `e87724c` (Slice 4 merged).

| Citation | Holds |
|---|---|
| `context.md`'s contracts this slice depends on: `design.md` §2's tables, the Shared contract's committed-evidence replay test (`hyalos-os/monitor/src/spike_evidence_tests.rs`, `python_committed_evidence_replays_on_both_backends`) | holds: the test runs every `expected.tsv` line from its inputs, so a hostile line needs no change to it |
| `hyalos-os/monitor/src/main.rs:205-229` the hostile-guest list | differs: the list is `PROBES` at `main.rs:385`; lines 205-229 are now `read_inputs` and the WAT `IMPORTS` |
| `hyalos-os/monitor/src/main.rs:459-473` the verdicts | holds, moved: `enum Verdict` at `main.rs:465` |
| `hyalos-os/monitor/src/runner.rs:633` the trace printer | holds: the call is at `:633`, `fn print_trace` at `:639` |
| `hyalos-os/ABI.md:70-71` the issue fixture's form | holds |
| `hyalos-os/monitor/src/main.rs:686` the KVM backend's 30-second limit | differs: `limit: Duration::from_secs(30)` is at `main.rs:708` |
| `hyalos-os/monitor/src/profile_tests.rs:114-121` the KVM gate | holds, moved: `gated_kvm_backend` at `:116`, `spike_kvm_backend` at `:114` |
| `.github/workflows/ci.yml:284-292` KVM and Landlock required | holds |
| `spikes/438/README.md`: `micropython profile:`, `monty profile:`, `process-only: micropython`, `process-only: monty` | holds: both runtimes built and both are process-only |
| `docs/plans/hyalos-kernel-python/decisions/`: the next free number | ADR-008 |
| `docs/plans/hyalos-kernel/pack-briefs.md`'s capsules section | holds |

No difference touches the slice's Contracts (none new; the replay test's form is unchanged).

## Acceptance

| ID | How it was shown |
|---|---|
| S-06.1 | Every case of `fixtures/hostile.tsv` ran under both runtimes to a result or a refusal (`hostile.sh` ends `hostile: ok` and stops on any other ending), and each run's reach is in `MEASUREMENTS.md`'s hostile table: labels asked, notification hash, result, fuel. The 16 records and refusals are in `expected.tsv` and replay (5.4) |
| S-06.1.e1 | Case `utf8`, four titles that are not UTF-8: `result ok 4 effects 1` under both, all four labelled security and copied into the notification byte for byte (the same body hash `40611b4c03734f86` under both) |
| S-06.1.e2 | Cases `record-16384` (a record of exactly 16,384 bytes) and `record-16385`: both `result ok 2 effects 1` under both runtimes; the first record is asked about (its first 4,096 bytes), the second is read as too large and counted other without asking (asked 2 and asked 1) |
| S-06.1.e3 | Case `collide`, one block of 200 titles per runtime from `fixtures/collide.py`: MicroPython `result ok 200` at 81,687,311 fuel against the distinct control's 81,687,923; Monty `refused OutOfFuel`, fuel recorded as the profile's 830,000,000, as is its control's. Both hashes were reproduced (`README.md`, Hostile inputs and measurements), so collisions were tested for both |
| S-06.2 | `MEASUREMENTS.md`'s issue fixture table: guest size, cold start, run time and fuel per runtime and backend, each with its command, times the median of three |
| S-06.2.e1 | Both runtimes built: their process rows have every figure and the command; their KVM rows give the `process-only:` reason in place of figures |
| S-06.2.e2 | Dry run: no runtime failed to build. `measure.sh` gives such a runtime a row `did not build: …` with `—` for every figure (its loop over `micropython` and `monty`, the branch on a missing guest or profile line); shown by running it with `guests/monty.wasm` moved aside (Verify, as run) |
| S-06.3 | `decisions/ADR-008-findings-adopt-micropython-monty-is-not-ready.md` names MicroPython, states the hostile-input limit an audit report must carry (its section of that name), and is accepted by the developer (5.3) |
| S-06.3.e1 | Both runtimes measured: the ADR names MicroPython, with reasons, and says why Monty is not ready and what it still needs. Dry run: every claim in it cites a `MEASUREMENTS.md` row, a record or `expected.tsv` line, a `README.md` line, or a progress file of Slices 3 and 4 |
| S-06.3.e2 | Does not apply: both runtimes built. The ADR still lists what Monty needs before adoption |
| 5.1 | `fixtures/hostile.tsv` holds S-06.1's examples plus `numbers`, `repr` (Monty's reading of bytes from their repr, Slice 4), `heap` (MicroPython's uncollected heap, Slice 3, and Monty's memory and fuel, Slice 4) and `distinct` (the collide blocks' control); its form is in its first lines. `python_5_1_5_4_every_hostile_case_is_listed_for_each_runtime` checks the e1 and e2 records' lengths and UTF-8, the 200-title blocks, and that each listed data file is the case's lines decoded. `collide.py --check` checks each block is 200 distinct 32-byte titles of one hash; the hash paths and lines are in `README.md` |
| 5.2 | `spikes/438/MEASUREMENTS.md`, printed by `measure.sh`; `measure.sh --check` passes (20 rows), and fails on a changed fuel figure or a time with no command (both tried) |
| 5.3 | ADR-008 states the monitor is the boundary (P3), names MicroPython and what Monty needs, states the audit-report limit (P5), lists what capsules must decide (the kernel image's dispatch, floats, the memory limit, fuel, the script field, type checking), and says signing the task identity was out of scope and is capsules'. `pack-briefs.md`'s capsules section cites it (M4). Its Status is set `accepted (developer, <date>)` in the PR's last commit, once the developer has approved the PR |
| 5.4 | The hostile records are committed under `spikes/438/evidence/` and listed in `expected.tsv`; `python_committed_evidence_replays_on_both_backends` reruns each from its inputs and replays each record. Shown to fail when a hostile outcome is changed (`monty-hostile-heap` listed `GuestTrap`: the test names the case, `Err("OutOfFuel")` against `Err("GuestTrap")`), and the listing test fails when a data file gains a byte |

### Verify, as run

Run on 2026-11-25, KVM and Landlock required (`HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1`):

- `spikes/438/hostile.sh`: `collide: micropython 200 titles, low 16 bits 3160`,
  `collide: monty 200 titles, low 16 bits c5fc`, then each case's outcome and reach, ending
  `hostile: ok`.
- `git diff --exit-code -- spikes/438/evidence`: exit 0, so a re-run leaves the evidence unchanged.
- `spikes/438/measure.sh`: exit 0. `measure.sh --check`:
  `measure: 20 rows match MEASUREMENTS.md (times not compared)`.
- Both `cargo fmt` checks and all four `cargo clippy` lines: exit 0. The first run flagged
  `byte_char_slices` in the new test; it is fixed, and clippy was run again.
- Both `attest` builds: exit 0.
- `cargo test … python_`: every binary ends `test result: ok`, with 24 passed in `task-kernel`'s
  and 3 in `release_switches`. `! grep '^skipped:'` passes, so nothing skipped.
- `grep -c -e '-hostile-' expected.tsv`: `16`, which is 8 cases for each of the 2 runtimes.
- The ADR `accepted` line: exit 1 until the developer approved the PR on 2026-11-25. After the
  PR's last commit it prints
  `ADR-008-…md:3:**Status:** accepted (developer, 2026-11-25)`.
- The brief's citation: `pack-briefs.md:102`.
- `cargo xtask lines` (hyalOS 9,347 / 12,000), `hyalos-unsafe` (`unsafe only in the 11 listed
  files`) and `docs` (`194 references in 3 pages`): exit 0.
- S-06.2.e2 dry run: with `guests/monty.wasm` moved aside, `measure.sh` printed
  `| monty | process | — | — | — | — | did not build: …` and the same for `kvm`. The guest was then
  put back.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Pattern to follow: `main.rs:205-229`, `:459-473`; Recorded blockers: `main.rs:686` | The hostile-guest list is `PROBES` at `main.rs:385`, `Verdict` at `:465`, the 30-second limit at `:708` | Slices 1 to 4 added code above them; the items are unchanged | n |
| Files: the data a case runs on is not listed | Each case's data file is committed under `spikes/438/evidence/data/`: `hostile-<case>.tsv` for a case tagged `all`, `<runtime>-hostile-<case>.tsv` for a runtime's block | The replay test runs each case from its inputs, so `expected.tsv` must name a data file that exists. `hostile.sh` writes them, so Verify's `git diff --exit-code -- spikes/438/evidence` covers them, and the listing test checks each is its case's lines | n |
| Files: modify `spike_evidence_tests.rs` to replay the hostile records | The replay test is unchanged, since it already runs every line; a new test, `python_5_1_5_4_every_hostile_case_is_listed_for_each_runtime`, checks `expected.tsv` against `hostile.tsv` | A hostile line has the same form as a Slice 3 or 4 line | n |
| 5.1: one case per runtime-specific weakness found in Slices 3 and 4 | No case for the kernel image's stack overflow (#503), nor for Monty's missing `bytearray` and `globals()` | #503 is reached only on KVM, and both runtimes are process-only, so no case runs there. A missing builtin is reached by a script, not by data, and the triage script is fixed | n |
| Verify: `hostile.sh` builds the kernel images it needs; a KVM `Timeout` makes a case process-only | It built none, and no case ended `Timeout` | Both runtimes have a `process-only:` line, so every case ran on the process backend; `README.md` says no hostile case has a `process-only:` line of its own | n |

## Decisions

- **Seed 0 for every hostile case.** At seed 0 the stub model labels a title by its words alone
  (`hyalos-os/monitor/src/main.rs`, `stub_model`), so a hostile title that says "security" is sure
  to be copied into the notification, and its bytes cross the guest boundary twice.
- **A collision is agreement in the low 16 bits.** That is MicroPython's whole hash. Monty's is
  64-bit SipHash-1-3, where 200 full collisions are out of reach, but a dict or set picks its first
  probe from the hash's low bits, so 16 bits collide every table of up to 65,536 buckets. Each
  block is 200 distinct titles of 32 lowercase letters; MicroPython's took 20 s to generate and
  Monty's 45 s, so `hostile.sh` runs `collide.py --check`, which hashes the committed block.
- **The reimplementations were checked against the runtimes,** not only read from source: a
  scratch script that finishes with `hash()` of six titles (including the empty one and a 32-byte
  one), run in each committed guest, gave the values `collide.py --hash` gives.
- **The fixture's escapes:** in a data line a backslash is always `\x5c` and a tab `\x09`, so a
  row has exactly three tab-separated fields and decoding is unambiguous. An escape that would put
  a newline in a record, or an empty record, is refused, since the loader would split or drop it.
- **Reach** is the labels asked (count per label), the first 16 hexadecimal digits of the
  notification body's SHA-256, the result and the fuel. A refused run has no record, so nothing it
  asked or sent is known.
- **MEASUREMENTS.md's times** are each the median of three `spike-run` runs, from its `timing:`
  line, taken on an Intel Core Ultra 9 185H under WSL2 on 2026-11-25. A KVM row gives the
  runtime's `process-only:` reason in the command column, with `—` for every figure.
- **The findings name MicroPython.** The one thing that stops it on KVM is the kernel image's
  dispatch (#503). Recommending a change to it would change a default build, so the ADR lists it
  under what capsules must decide (the slice's last stop condition), with the local run that shows
  the fix works.
- **ADR-008 was accepted by the developer on 2026-11-25,** who approved the PR. Its Status was
  set in the PR's last commit (5.3).

## Discoveries

- **Fuel is not time.** Monty's 413,707,242-fuel triage run takes about 72 ms and MicroPython's
  33,127,745 about 14 ms (`MEASUREMENTS.md`): 12.5 times the fuel for about 5 times the time. A
  fuel limit chosen for time would be wrong by the same factor. What `wasmi` charges for in
  Monty's run was not measured.
- **MicroPython's heap, never collected, is a limit on inputs.** 200 titles of 4,096 bytes end
  `error MemoryError` after 93 questions, with no notification (`heap`). Hostile data can deny the
  task's one effect.
- **Monty cannot triage 200 issues under `hyalos.spike/2`.** Two hundred 32-letter titles run out
  of its 830,000,000 fuel (`distinct`, `collide`), while 40 issues need about 414 million (R8).
- **Monty's repr round trip held** for both quotes, backslashes, `\x41` and `\n` as text, a tab,
  a carriage return, a NUL and high bytes: its notifications and results equal MicroPython's on
  `repr` and `utf8`.
- **A title reaches the notification byte for byte** when labelled security, including bytes
  that are not UTF-8 and a carriage return that draws a forged line on a terminal. In a live run a
  title that is not UTF-8 is refused `ModelRefused` (`hyalos-os/ABI.md`), so `utf8` holds for
  recorded runs only.
- **Collisions had no effect** because the triage script never hashes a title. Both hashes are
  fixed by their source and seed, so a task that keys a dict or set by its inputs would be open to
  them; `collide.py` aims at both.
