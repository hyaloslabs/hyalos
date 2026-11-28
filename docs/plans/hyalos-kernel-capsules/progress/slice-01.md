# Slice 1 — Long guest loops run on KVM

| | |
|---|---|
| **Status** | done |
| **Issue** | #508 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-28 |

## Preflight

Checked on the branch's base, `99d3b9f` (the pack's PR merged).

| Citation | Holds |
|---|---|
| `hyalos-os/kernel-image/Cargo.toml:11` `wasmi` 2.0.3 with `stable`, `validate`, `memory64`, `auto-dispatch`, `libm` | holds |
| `hyalos-os/review/scope.toml:29` the `wasmi` row's reason names `auto-dispatch` | holds |
| `hyalos-os/monitor/src/kvm.rs:29-41` RAM 32 MiB, the stack `0x1e0_0000..RAM` | holds |
| `spikes/438/README.md:99` the `process-only: micropython` line | holds |
| `spikes/438/micropython/run.sh:61-63` the case loop, skipping `*-hostile-*`; `:64-70` legs and the outcome check; `:78` the `<id> <leg>: <outcome>` line; `:84-90` the `KernelFault` check; `:91-93` the no-KVM branch that only echoes | holds |
| `spikes/438/evidence/expected.tsv:2-5`, `:11-18` the twelve MicroPython rows, each `process` | holds |
| `spikes/438/hostile.sh:121-126` the `process-only:` lookup; `:156` the `<name> <leg>: <outcome>` line; `:161` the records compared; `:190-198` the check against `expected.tsv` | holds |
| `spikes/438/measure.sh:20` `set -euo pipefail`; `:44` `process_only`; `:52`, `:56` `\| head -2`; `:82` the `process-only` row | holds |
| `spikes/438/MEASUREMENTS.md:23` the MicroPython `kvm` row, `process-only` | holds |
| `hyalos-os/monitor/src/spike_evidence_tests.rs:169` `python_committed_evidence_replays_on_both_backends` | holds |
| `hyalos-os/monitor/src/spike_tests.rs:13` `PROFILE_2_BASE`; `:37-52` its pins on both backends | holds |
| `hyalos-os/monitor/src/ring_tests.rs:15` `BASE` | holds |
| `hyalos-os/monitor/src/profile_tests.rs:99` `kvm_backend` | holds |
| `hyalos-os/monitor/src/abi_tests.rs:162-177` `on_both` | holds |
| `hyalos-os/audit/src/replay.rs:51-65` `ReplaySource::answer`, `ReplayDiverged` on a different digest | holds |
| `hyalos-os/monitor/src/main.rs:1002-1030` the test modules | holds |
| `.github/workflows/ci.yml:266-269`, `:290-299`, `:363-373` the KVM jobs, `HYALOS_REQUIRE_KVM` | holds |
| `wasmi` 2.0.3 has a `portable-dispatch` feature, which overrides `auto-dispatch` (`wasmi/src/lib.rs:70-71`, `src/engine/executor/handler/dispatch/mod.rs:1-12`) | holds |

## Acceptance

The executor's host has `/dev/kvm`, but the device does not open (mode `0600`, owner root), and
the KVM tests must not run on it anyway: they crashed this WSL2 VM before. Every KVM gate below
skipped here, so a skip is not a pass. The KVM legs are proven by CI and by the developer (Shared
contract, KVM steps without `/dev/kvm`; ADR-023, ADR-036).

| ID | How it was shown |
|---|---|
| S-01.1 | S-01.1.e1 and S-01.1.e2 |
| S-01.1.e1 | `abi_tests::capsules_s_01_1_e1_a_10_000_iteration_loop_ends_as_on_the_process_backend`: a one-loop guest counting to 10,000 gives `[10000]`, and `on_both` demands the KVM transcript equal the process one. Process leg: passed here. KVM leg: CI `hyalos-os` job (below) |
| S-01.1.e2 | `abi_tests::capsules_s_01_1_e2_a_1_000_000_iteration_loop_never_ends_kernel_fault`: the same guest at 1,000,000 iterations gives the same outcome on both backends, which must be `[1000000]` or `OutOfFuel`, never `KernelFault`. Process leg: passed here. KVM leg: CI |
| S-01.2 | S-01.2.e1 and S-01.2.e2 |
| S-01.2.e1 | `spike_evidence_tests::python_committed_evidence_replays_on_both_backends`, unchanged, now runs every MicroPython case on KVM too, since each is listed `both`: each transcript must equal the committed record and replay. Process legs: passed here. KVM legs: CI `hyalos-os-spike` job |
| S-01.2.e2 | `spike_evidence_tests::capsules_s_01_2_e2_a_changed_question_digest_diverges_on_both_backends`: `micropython-triage-0.json` with the first hex digit of answer 0's `question_sha256` changed (`0` to `1`, any other digit to `0`) still parses, and replay ends `Err(ReplayDiverged)`. Process leg: passed here. KVM leg: CI |
| S-01.3 | S-01.3.e1 and S-01.3.e2; no pinned hash, `fuel_used` or committed record changed (`git diff origin/main -- spikes/438/evidence/*.json` is empty) |
| S-01.3.e1 | `spike_tests::capsules_s_01_3_e1_profile_2_pins_hold_on_the_rebuilt_image`: `PROFILE_2_BASE`'s hash and `fuel_used`, for seeds 0 and 7, on KVM with the rebuilt image. KVM only: CI |
| S-01.3.e2 | `ring_tests::capsules_s_01_3_e2_profile_1_pins_hold_on_the_rebuilt_image`: `BASE`'s hashes under `hyalos.profile/1` on KVM. KVM only: CI |
| S-01.4 | S-01.4.e1 and S-01.4.e2; `run.sh` and `hostile.sh` on bot1's KVM (KVM, on the developer's host) |
| S-01.4.e1 | `spike_evidence_tests::capsules_s_01_4_e1_every_micropython_case_is_listed_both`: 12 MicroPython rows, 8 of them hostile, all `both`; each result names a committed record that exists; `micropython-raw-effect` names none. Passed here. The same refusal on both backends: `python_committed_evidence_replays_on_both_backends`'s KVM leg (CI) and `run.sh`'s two `micropython-raw-effect … refused UnknownEffect` lines (bot1) |
| S-01.4.e2 | Dry run with `auto-dispatch` restored, on bot1: `run.sh` exits 1 on `micropython-triage-0 on kvm: 'refused KernelFault'` (KVM, on the developer's host) |
| 1.1 | `git grep -c auto-dispatch -- hyalos-os/review/scope.toml hyalos-os/kernel-image/Cargo.toml`: no output, exit 1. `git grep -c portable-dispatch` on the same files: `hyalos-os/kernel-image/Cargo.toml:1`, `hyalos-os/review/scope.toml:1`. `cargo xtask review-scope`: `hyalOS review scope: 50 rows agree with 3 lockfiles` |
| 1.2 | `grep -c "process-only: micropython" spikes/438/README.md`: `0` |

### Verify, as run by the executor

From the repository root, without `HYALOS_REQUIRE_KVM` (the KVM gates skipped):

- Both `attest` builds: exit 0.
- `cargo test … --features …,spike-contract … -- capsules_ python_committed_evidence`: the seven tests above, `test result: ok. 7 passed`.
- The workspace run with the spike-contract features: every binary `test result: ok` except
  `ledger::fixture_tests::local_4_2_generate`, which fails here on a refusal this slice does not touch
  (Discoveries).
- `awk … $8 == "both"` over `expected.tsv`: 4 non-hostile, 12 in all, 0 not `both`; the record check: 0.
- `git diff --exit-code origin/main -- hyalos-os/kernel-image/Cargo.lock`: unchanged, after building the image with `cargo build --release --locked`.
- `cargo fmt --check` (workspace and kernel image), the three workspace clippy runs (default, test features, spike-contract features) and the two kernel image clippy runs: exit 0.
- `cargo xtask lines`: `hyalOS: 9347 / 12000 lines (77.9%)`. `hyalos-unsafe`: `unsafe only in the 11 listed files, each use with a SAFETY comment`. `docs`: `docs ok: 194 references in 3 pages`. `layout`: `layout ok: 21 workspace crates`. `review-scope`: `50 rows agree with 3 lockfiles`. `review-package`: `review package ok: 7 sections, commit under review is an ancestor of HEAD, 31 code references`.

### KVM, by CI

CI run `46109858053`, on the slice's code commit `283e0e9` (the commits after it change only this
file):

```
$ gh run view 46109858053 --json conclusion,headSha --jq '.conclusion + " " + .headSha'
success 283e0e9d960bd045b81681e17506ca1cc0c1ee28
$ git rev-parse HEAD
283e0e9d960bd045b81681e17506ca1cc0c1ee28
```

Both jobs set `HYALOS_REQUIRE_KVM=1`, so a test that could not reach KVM would have panicked, and
neither log has a `skipped:` line. The `hyalos-os` job (`140870807140`) ran
`capsules_s_01_1_e1_…`, `capsules_s_01_1_e2_…`, `capsules_s_01_3_e1_…` and `capsules_s_01_3_e2_…`,
each `ok`. The `hyalos-os (spike contract)` job (`140870807155`) ran those four and
`capsules_s_01_2_e2_…`, `capsules_s_01_4_e1_…` and `python_committed_evidence_replays_on_both_backends`,
each `ok`: every MicroPython case, the hostile ones included, gave its committed record on KVM and
replayed there (S-01.2.e1, S-01.4.e1's same refusal for `micropython-raw-effect`).

### KVM, on the developer's host

The developer asked the executor to run these steps on bot1, their KVM host, on 2026-11-28
(Deviations). bot1 is an Intel N150, 4 CPUs, Ubuntu 24.04, Linux 6.8.0-147. Its user is not in the
`kvm` group, so each step ran in a container holding the pinned toolchain (Rust 1.99.1, with the
`x86_64-unknown-none` and `wasm32-unknown-unknown` targets), on Docker 29.2.1, with the host's
device passed in (`docker run --device /dev/kvm --group-add <kvm gid> --user 1000:1000`), over a
fresh clone of this branch at `c50fd7a`. Inside it `/dev/kvm` opened read-write.

Step 1, S-01.4:

```
$ HYALOS_REQUIRE_KVM=1 spikes/438/micropython/run.sh > run.log 2>&1; echo exit=$?
exit=0
micropython-triage-0 process: result ok 40 effects 1
micropython-triage-0 kvm: result ok 40 effects 1
micropython-triage-7 process: result ok 40 effects 1
micropython-triage-7 kvm: result ok 40 effects 1
micropython-no-notify process: result error AttributeError effects 0
micropython-no-notify kvm: result error AttributeError effects 0
micropython-raw-effect process: refused UnknownEffect
micropython-raw-effect kvm: refused UnknownEffect
micropython-triage-0 kvm: result ok 40 effects 1
micropython: ok
$ grep -c '^micropython-raw-effect' run.log; grep -c ' kvm: ' run.log
2
5
```

Step 2, S-01.4:

```
$ spikes/438/hostile.sh > hostile.log 2>&1; echo exit=$?
exit=0
$ grep '^micropython-hostile-.* kvm:' hostile.log
micropython-hostile-utf8 kvm: result ok 4 effects 1
micropython-hostile-record-16384 kvm: result ok 2 effects 1
micropython-hostile-record-16385 kvm: result ok 2 effects 1
micropython-hostile-numbers kvm: result ok 8 effects 1
micropython-hostile-repr kvm: result ok 5 effects 1
micropython-hostile-heap kvm: result error MemoryError effects 0
micropython-hostile-distinct kvm: result ok 200 effects 1
micropython-hostile-collide kvm: result ok 200 effects 1
$ grep -c '^micropython-hostile-.* kvm: process-only' hostile.log; tail -n 1 hostile.log
0
hostile: ok
$ git diff --exit-code -- spikes/438/evidence; echo $?
0
```

Monty's eight hostile lines still print `kvm: process-only`, as its README line says.

Step 3: `spikes/438/measure.sh` printed MicroPython's `kvm` row, `| micropython | kvm | 249,874 |
21.0 | 69.0 | 33,127,745 | …`, the same fuel as the process row. That row replaced
`MEASUREMENTS.md:23`, with the host it was taken on named above the table. Then:

```
$ for i in 1 2 3 4 5 6 7 8 9 10; do spikes/438/measure.sh --check >/dev/null 2>&1; echo "$?"; done
0  (ten lines, each 0)
$ spikes/438/measure.sh --check
measure: 20 rows match MEASUREMENTS.md (times not compared)
$ grep -c -E '^\| micropython \| kvm \| [0-9]' spikes/438/MEASUREMENTS.md
1
```

Step 4, S-01.4.e2, the dry run: `portable-dispatch` changed back to `auto-dispatch` in
`hyalos-os/kernel-image/Cargo.toml`, then:

```
$ HYALOS_REQUIRE_KVM=1 spikes/438/micropython/run.sh; echo exit=$?
micropython-triage-0 process: result ok 40 effects 1
run.sh: micropython-triage-0 on kvm: 'refused KernelFault', expected 'result ok 40 effects 1'
exit=1
```

The image was then restored (`git checkout -- hyalos-os/kernel-image/Cargo.toml`,
`spikes/438/kernel.sh hyalos.spike/1`), and `HYALOS_REQUIRE_KVM=1 spikes/438/micropython/run.sh`
again ended `micropython-triage-0 kvm: result ok 40 effects 1` and `micropython: ok`. The clone's
only change was `MEASUREMENTS.md`, the same as this branch's.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `MEASUREMENTS.md`'s MicroPython `kvm` row gets figures from `measure.sh`, on the same host as the other rows | The row came from bot1, a different host from the other rows' (2026-11-25, WSL2), which keep their times | Its times come only from a KVM run, and the executor's host cannot run one. The paragraph above the table names both hosts, so the two MicroPython times are not read as comparable | n |
| The developer runs the KVM steps and sets **Status** `done` | The executor ran them on bot1, the developer's KVM host, in a container given `/dev/kvm`, and set `done` | The developer asked for it in this session. Each step's command and output are above, under KVM, on the developer's host | n |
| Suggested path 3: S-01.3's tests run the pins on KVM | They run on KVM only, and the process leg is left to the existing pin tests | `python_s_01_4_e2_profile_2_transcripts_hold` and `core_s_02_1_e1_…` already compare both backends; the new tests name the criterion and check the rebuilt image | n |

## Decisions

- S-01.1's guest is a counted loop over a local (`i32.add`, `br_if`), the shape of #503's C guest, in `abi_tests.rs` beside `on_both`. A bare `(loop (br $l))` already ran to `OutOfFuel` on KVM before this slice (`core_s_02_1_e2`), so it would not have caught #503.
- `run.sh`'s KVM check compares the triage case's KVM outcome with `expected.tsv`'s row, not only with `KernelFault`, so any other wrong outcome fails too.
- `scope.toml`'s `wasmi` reason says why the portable dispatch is used without naming the old feature, so 1.1's `git grep -c auto-dispatch` finds nothing.

## Discoveries

- `ledger::fixture_tests::local_4_2_generate` fails on this host: one
  generated refusal reads `answer 6: the session is later than the run's result` where the committed
  manifest has `answer 7`. The case (`forgery_tests.rs:589-595`, S-03.3.e3) depends on which
  session's wall-clock second falls after the signed result, so the generated fixture depends on
  timing. It passed in both CI jobs of run `46109858053`, so it is this host's timing, not this
  slice.
- Monty's `process-only:` reason (`spikes/438/README.md:140`, and `MEASUREMENTS.md`'s Monty `kvm`
  row) still says a layout that held its guest "would still leave the kernel stack overflow of issue
  #503". This slice fixes that overflow, so that half of the reason is now out of date. Its first half,
  the guest being too big for `GUEST_MAX`, still holds. This slice does not touch Monty's line.

## Blocked on

Nothing.
