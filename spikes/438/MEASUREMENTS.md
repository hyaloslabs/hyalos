# Measurements of the Python guest spike

Slice 5 of `docs/plans/hyalos-kernel-python` (S-06.2, 5.2). `spikes/438/measure.sh` prints both
tables below, and `measure.sh --check` measures again and stops unless every figure but the times
is the same. The times vary from run to run and from host to host, so they are recorded here, not
reproduced. Each is the median of three runs of the row's command, as `spike-run` prints it on its
`timing:` line: the cold start is from the command's start to the guest's first host call, and the
run time is from the start to the end of the run. Both include reading the guest and starting
`wasmi`, and neither includes writing the record. They were taken on 2026-11-25 on an Intel Core
Ultra 9 185H, 10 CPUs, under WSL2 (Linux 6.6.87.2), with the release build `measure.sh` makes,
except MicroPython's `kvm` row (Slice 1 of `docs/plans/hyalos-kernel-capsules`), taken on
2026-11-28 on an Intel N150, 4 CPUs (Linux 6.8.0), on its KVM, so its times are not comparable
with the process row's.

## The issue fixture

Each runtime that built runs the triage task on `hyalos-os/evidence/fixtures/issues.tsv`, 40
issues, at seed 0, under its profile (`spikes/438/README.md`): MicroPython under `hyalos.spike/1`,
Monty under `hyalos.spike/2`. MicroPython runs on both backends, on the default kernel image,
since it builds `wasmi` with `portable-dispatch` (issue #503). Monty is process-only, so its KVM
row gives the reason `spikes/438/README.md` records for it in place of figures. Fuel and time are not in proportion:
Monty uses 12.5 times MicroPython's fuel and about 5 times its run time.

| Runtime | Backend | Guest bytes | Cold start (ms) | Run time (ms) | Fuel | Command |
|---|---|---|---|---|---|---|
| micropython | process | 249,874 | 2.3 | 13.9 | 33,127,745 | `HYALOS_SPIKE_PROFILE=hyalos.spike/1 hyalos-os/target/spike/release/task-kernel spike-run spikes/438/guests/micropython.wasm spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py hyalos-os/evidence/fixtures/issues.tsv out.json 0` |
| micropython | kvm | 249,874 | 21.0 | 69.0 | 33,127,745 | `HYALOS_SPIKE_PROFILE=hyalos.spike/1 HYALOS_KERNEL=$(spikes/438/kernel.sh hyalos.spike/1) hyalos-os/target/spike/release/task-kernel spike-run spikes/438/guests/micropython.wasm spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py hyalos-os/evidence/fixtures/issues.tsv out.json 0` |
| monty | process | 6,945,056 | 36.8 | 71.5 | 413,707,242 | `HYALOS_SPIKE_PROFILE=hyalos.spike/2 hyalos-os/target/spike/release/task-kernel spike-run spikes/438/guests/monty.wasm spikes/438/contracts/draft spikes/438/sdk/draft.py spikes/438/scripts/triage.py hyalos-os/evidence/fixtures/issues.tsv out.json 0` |
| monty | kvm | — | — | — | — | process-only: does not fit KVM: the guest is 6,945,056 bytes against `GUEST_MAX` 1 MiB (1,048,576 bytes), so the `spike-profile-2` image refuses it with `TooLarge` before boot. Its memory, 5 MiB, would fit the kernel's 16 MiB heap. No spike-only layout was built: one that held the guest would still leave the kernel stack overflow of issue #503, which ends any guest loop of about 10,000 iterations in `KernelFault`, and Monty's run is about 400 million fuel. That reason is inferred from #503, not shown on Monty. |

## The hostile inputs

One row per hostile case of `spikes/438/fixtures/hostile.tsv` and runtime, from its committed
record under `spikes/438/evidence/` (`spikes/438/hostile.sh` runs them, at seed 0, on the process
backend, and MicroPython's on KVM as well, with the same record). Asked is the number of questions and how many got
each label; the notification is the first 16 hexadecimal digits of the SHA-256 of its body. A
refused run writes no record, so nothing it asked or sent is known; for `OutOfFuel` the fuel is
the profile's, which the run used up.

| Case | Runtime | Outcome | Asked (bug, feature, security, other) | Notification SHA-256 | Result | Fuel |
|---|---|---|---|---|---|---|
| utf8 | micropython | result ok 4 effects 1 | 4 (0, 0, 4, 0) | `40611b4c03734f86` | `ok 4` | 21,844,884 |
| record-16384 | micropython | result ok 2 effects 1 | 2 (0, 0, 0, 2) | `c84b8b9cc46ada1a` | `ok 2` | 21,995,559 |
| record-16385 | micropython | result ok 2 effects 1 | 1 (0, 0, 0, 1) | `c84b8b9cc46ada1a` | `ok 2` | 20,544,247 |
| numbers | micropython | result ok 8 effects 1 | 1 (0, 0, 1, 0) | `84a454bd0c977f03` | `ok 8` | 22,067,620 |
| repr | micropython | result ok 5 effects 1 | 5 (0, 0, 5, 0) | `205e63a604ebd729` | `ok 5` | 22,282,913 |
| heap | micropython | result error MemoryError effects 0 | 93 (0, 0, 0, 93) | `none` | `error MemoryError` | 152,753,977 |
| distinct | micropython | result ok 200 effects 1 | 200 (0, 0, 0, 200) | `9162154afd22c94f` | `ok 200` | 81,687,923 |
| collide | micropython | result ok 200 effects 1 | 200 (0, 2, 0, 198) | `781187f666c1a7c0` | `ok 200` | 81,687,311 |
| utf8 | monty | result ok 4 effects 1 | 4 (0, 0, 4, 0) | `40611b4c03734f86` | `ok 4` | 78,741,242 |
| record-16384 | monty | result ok 2 effects 1 | 2 (0, 0, 0, 2) | `c84b8b9cc46ada1a` | `ok 2` | 65,581,406 |
| record-16385 | monty | result ok 2 effects 1 | 1 (0, 0, 0, 1) | `c84b8b9cc46ada1a` | `ok 2` | 48,873,500 |
| numbers | monty | result ok 8 effects 1 | 1 (0, 0, 1, 0) | `84a454bd0c977f03` | `ok 8` | 94,064,804 |
| repr | monty | result ok 5 effects 1 | 5 (0, 0, 5, 0) | `205e63a604ebd729` | `ok 5` | 88,840,281 |
| heap | monty | refused OutOfFuel | — | — | — | 830,000,000 |
| distinct | monty | refused OutOfFuel | — | — | — | 830,000,000 |
| collide | monty | refused OutOfFuel | — | — | — | 830,000,000 |
