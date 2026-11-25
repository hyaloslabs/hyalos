# Slice 1 — The monitor enforces a draft contract

| | |
|---|---|
| **Status** | done |
| **Issue** | #494 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-25 |

## Preflight

Checked against the branch's base, `6112dd0`, the commit `context.md` cites as `069f31c` plus the
pack's own PR, which touched only `docs/plans/`.

| Citation | Holds |
|---|---|
| `hyalos-os/audit/src/replay.rs:118-124` `pre_infer` accepts schema `0` only | holds |
| `hyalos-os/audit/src/replay.rs:144-153` `pre_effect` accepts kind `0`, one effect | holds |
| `hyalos-os/audit/src/replay.rs:236-246` the check runs before `guest_bytes` | holds |
| `hyalos-os/audit/src/replay.rs:378-404` `execute_with` | holds |
| `hyalos-os/audit/src/record.rs:21-52` the closed `Reason` set | holds |
| `hyalos-os/audit/src/record.rs:150-189`, `:205`, `:256-276` `read_transcript`, `task_v4`, `framed_task` | holds |
| `hyalos-os/profile/src/lib.rs:35-71`, `:87-93`, `:97-101` profiles, `KNOWN`, `same_settings`, `lookup` | holds |
| `hyalos-os/monitor/src/main.rs:155-171` `on_backend` and its `same_settings` check at `:165` | holds |
| `hyalos-os/monitor/src/main.rs:182-189` `read_inputs` splits on newlines | holds |
| `hyalos-os/monitor/src/main.rs:205` `attacks()` | holds |
| `hyalos-os/monitor/src/main.rs:682-689` `load_backend`, 30-second limit at `:686` | holds |
| `hyalos-os/monitor/src/main.rs:903` the `task` command | holds |
| `hyalos-os/monitor/src/runner.rs:65-95` `REASONS`, 29 entries | holds |
| `hyalos-os/monitor/src/kvm.rs:29-41` the machine layout; `:328-355` `Calls` and its `Host` impl | holds |
| `hyalos-os/monitor/Cargo.toml:24-31` `test-trust`, `test-switches` | holds |
| `hyalos-os/monitor/tests/release_switches.rs:63-115`, `:82` `built_kernel` | holds |
| `hyalos-os/monitor/src/abi_tests.rs:103-146` `every_reason`; `:161-177` `on_both` | holds |
| `hyalos-os/monitor/src/runner_tests.rs:464-500`, `:584` the pinned reason table | holds |
| `hyalos-os/monitor/src/profile_tests.rs:114-121` the KVM gate | holds |
| `hyalos-os/monitor/src/ring_tests.rs:12-14`, `:58` profile 1's pin | holds |
| `.github/workflows/ci.yml:68` change filter; `:233-325` the `hyalos-os` job | holds |
| `crates/xtask/src/hyalos.rs:14` budget 12,000; `:18` `UNSAFE_FILES` | holds |

## Acceptance

Tests are in `hyalos-os/monitor/src/spike_tests.rs` unless named otherwise. Every both-backend
test ran on KVM here (`/dev/kvm`, `HYALOS_REQUIRE_KVM=1`), with no `skipped:` line.

| ID | How it was shown |
|---|---|
| S-01.1 | `python_s_01_1_e1_…`, `python_s_01_1_e2_…` |
| S-01.1.e1 | `python_s_01_1_e1_an_ungranted_effect_is_refused_and_never_sent`: `UnknownEffect` on both backends, the source saw 0 effects |
| S-01.1.e2 | `python_s_01_1_e2_a_granted_effect_is_recorded`: one effect recorded, one reached the source |
| S-01.2 | `python_s_01_2_e1_…`, `python_s_01_2_e2_…` |
| S-01.2.e1 | `python_s_01_2_e1_an_ungranted_schema_is_refused_and_never_asked`: `UnknownSchema`, 0 questions reached the source |
| S-01.2.e2 | `python_s_01_2_e2_an_unknown_schema_is_refused_as_without_a_contract`: `UnknownSchema` under `hyalos.spike/1` and under `hyalos.profile/2` |
| S-01.3 | `python_s_01_3_e1_…` to `_e3_…`, plus `python_1_5_…` for the commands |
| S-01.3.e1 | `python_s_01_3_e1_a_record_0_that_is_not_a_contract_is_refused_before_the_guest_loads`: `BadContract` with a guest that is not a module, so the refusal came before loading |
| S-01.3.e2 | `python_s_01_3_e2_no_input_records_is_refused`: `BadContract` |
| S-01.3.e3 | `python_s_01_3_e3_a_contract_with_an_unknown_field_is_refused`: `BadContract`; the parser's own cases in `hyalos-os/audit/src/contract.rs` (`python_anything_else_is_no_contract`) |
| S-01.4 | the rows below, plus `python_s_01_4_a_default_build_has_no_spike_command_and_ignores_its_variable` (`hyalos-os/monitor/tests/release_switches.rs`, run in both builds) |
| S-01.4.e1 | `python_s_01_4_e1_a_default_build_refuses_a_spike_record` (monitor) and `python_s_01_4_e1_a_default_build_knows_no_spike_profile` (`hyalos-os/profile/src/lib.rs`), both compiled only without the feature |
| S-01.4.e2 | `python_s_01_4_e2_profile_2_transcripts_hold`, pinned at `6112dd0` (1.6), passing in both workspace runs; `core_s_02_1_e1_…` (profile 1) passing in the feature run |
| S-01.5 | `python_s_01_5_e1_…`, `python_s_01_5_e2_…` |
| S-01.5.e1 | `python_s_01_5_e1_a_spike_record_replays_on_both_backends` |
| S-01.5.e2 | `python_s_01_5_e2_a_record_whose_contract_drops_its_effect_fails_replay`: `UnknownEffect` on both |
| S-03.1 | `python_s_03_1_e1_…`, `python_s_03_1_e2_…` |
| S-03.1.e1 | `python_s_03_1_e1_each_file_reaches_the_guest_as_one_whole_record`: records 0 contract, 1 SDK, 2 the 40-line script whole, 3 to 5 the data lines; a guest that returns record 2 returns every byte on both backends |
| S-03.1.e2 | `python_s_03_1_e2_an_empty_script_is_refused_before_the_run` |
| S-03.2 | `python_s_03_2_e1_…` |
| S-03.2.e1 | `python_s_03_2_e1_the_task_is_version_4_framing_over_every_record`: version 4, `task` equals `task_v4("hyalos.spike/1", guest, records)` |
| S-03.3 | `python_s_03_3_e1_…` |
| S-03.3.e1 | `python_s_03_3_e1_one_changed_script_byte_is_refused`: `read_transcript` refuses |
| 1.1 | `python_1_1_the_hostile_guests_end_as_listed_with_the_feature`: every `attacks()` case under `hyalos.profile/2`, both backends |
| 1.2 | `docs/adr/0136-hyalos-spike-contract-check.md` and its row in `docs/adr/README.md`; `ABI.md` untouched. Proposed in review, Accepted in the PR's last commit |
| 1.3 | `.github/workflows/ci.yml`: a job `hyalos-os (spike contract)` lints and tests the workspace with the three `spike-contract` features, gathered into `build, lint, test` like `hyalos-os`, which still lints and tests without them; the change filter matches `spikes/438/` |
| 1.4 | `cargo xtask lines`: `hyalOS: 9287 / 12000 lines`; `cargo xtask hyalos-unsafe` passes (no new `unsafe`) |
| 1.5 | `python_1_5_spike_task_equals_record_task` (`hyalos-os/monitor/tests/release_switches.rs`), against the built binary |
| 1.6 | Before any change, `task-kernel run guest/digest.wasm evidence/fixtures/issues.tsv` under `hyalos.profile/2` at `6112dd0` on both backends: seed 0 `0cb6dd27…ca2a`, fuel 29,845; seed 7 `fa92e457…87de`, fuel 30,886; process and KVM byte-identical. Pinned in `PROFILE_2_BASE` |

### Verify, as run

On this host (WSL2, `/dev/kvm` and Landlock available), from the repository root:

- `cargo fmt` (workspace and kernel image): clean. `cargo clippy … -D warnings`: clean with the
  test features, with them and the three `spike-contract` features, and with none; the kernel
  image's two clippy runs clean.
- Both `attest` builds succeeded.
- `HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1 cargo test --release --locked --workspace` with
  the test features: exit 0, 466 passed, 0 failed, 326 s. With the `spike-contract` features too:
  exit 0, 483 passed, 0 failed, 272 s. `release_switches` without features: 6 passed.
  `grep '^skipped:'` over all three logs: no match.
- `--list` counts: `python_s_0` with the feature 15 (at least 14); `python_s_01_4_e1` without it 2
  (at least 1); `python_1_5_` with it 1.
- `grep -c spike-contract .github/workflows/ci.yml`: 4.
- `cargo xtask lines`: `hyalOS: 9287 / 12000 lines (77.4%)`. `cargo xtask hyalos-unsafe`: unsafe
  only in the 11 listed files. `cargo xtask docs`: ok. `cargo xtask layout`: ok.
- The ADR-0136 index line reads Accepted after the PR's last commit.

Dry run (S-03.1.e1, S-03.2.e1, S-01.5.e1): `task-kernel spike-run guest/digest.wasm contract
sdk.py script.py evidence/fixtures/issues.tsv out.json`, with the draft contract, a two-line SDK
and a one-line script, gave `ok: 43 inputs, 40 inferences, 1 effect, fuel 31536` on the process
backend and on KVM (`HYALOS_KERNEL` set to the default image), with byte-identical records naming
`hyalos.spike/1`, version 4, whose first three inputs are the three files' bytes. `replay` printed
`replay identical` on both backends. Timing: `cold_start 0 ms, run_time 0 ms` on the process
backend and `cold_start 44 ms, run_time 51 ms` on KVM.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Suggested path 7: add the feature runs to CI's `hyalos-os` job | They are a job of their own, `hyalos-os (spike contract)`, gathered into the required check like `hyalos-os` | In one job the two test runs exceeded its 20-minute limit; `ci.yml` says to split work rather than raise a limit | n |
| Suggested path 3: unit-test S-01.3, S-03.2 and S-03.3 in `hyalos-audit` | They are tested in the monitor's `spike_tests.rs`, on both backends; `hyalos-audit` unit-tests only the contract parser | The monitor's tests reach both backends and the real `spike-run` input reader, which a unit test in the audit cannot | n |
| `design.md` §2 Spike timing: `cold_start` and `run_time` | The timing line also names the backend: `timing: cold_start N ms, run_time M ms, backend process\|kvm` | Slice 5's `measure.sh` compares backends, and the line then says which it measured | n |
| `context.md` §2: the contract read lives in `execute_with` | As planned; the first host call's time is kept on `Host` (`first_call`, behind the feature) and read back by `spike-run` from the host its backend returns | `Host` is the one place both backends answer through, so the timing is measured the same way on both | n |

## Decisions

- **How a profile enforces a contract.** `Profile::enforces_contract()` (behind the feature) is true
  for every `hyalos.spike/` name, rather than a new `Profile` field: a field would change every
  profile's value and `same_settings`, and the kernel image's copy of the struct.
- **An unreadable contract file** is passed to the run as an empty record 0, so `execute_with`
  refuses it `BadContract` like any other non-contract, as `design.md` §2 asks; an unreadable SDK,
  script or data file is a usage error before the run.
- **`spike-task` on a bad contract** prints `refused: BadContract` (the monitor's usual refusal line)
  and exits non-zero.
- **The seed** of `spike-run` must be decimal digits only; Rust's `u64` parser alone would accept
  `+7`.
- **CI** runs the whole workspace's tests a second time with the feature, not a subset, so the pinned
  transcripts and every other test are shown unchanged by it. It runs in its own job (below).

## Discoveries

- For Slice 2 and later: `spike-run`'s argument order is `<guest> <contract> <sdk> <script> <data>
  <record> [seed]`, and `HYALOS_SPIKE_PROFILE` must name a `hyalos.spike/` profile this build knows,
  or the run is refused before it reads anything but its arguments.
- `runner::REASONS` is 30 entries in a spike build; the channel fuzzer's `% 29` index still covers
  only the first 29, which is harmless (every index is valid) but never sends `BadContract`.
- Whole-millisecond timing read `0 ms` for the digest guest on the process backend (the dry run
  above), so after review the timing line prints milliseconds to three decimals
  (`cold_start 0.412 ms`), which Slice 5's measurements can use.
- In one job, the two test runs took CI's `hyalos-os` past its 20-minute limit (20m16s on the PR's
  first green-test run: 9 minutes without the feature, 8 with it, then the verifier rebuild was
  cancelled). The feature's lint and tests moved to their own job, `hyalos-os (spike contract)`,
  which is the repository's rule for a job near its budget. Later slices' evidence replay runs there.
