# Slice 2 — Framed inputs and the draft `hyalos.abi/2` section

| | |
|---|---|
| **Status** | blocked |
| **Issue** | #509 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-28 |

## Preflight

Checked on the branch's base, `b2ee4b0` (Slice 1 merged).

| Citation | Holds |
|---|---|
| `hyalos-os/monitor/src/main.rs:199-206` `read_inputs`, the newline split, `cannot read inputs` | holds |
| `main.rs:726`, `:761`, `:911`, `:929`, `:945`, `:953` the callers in `run`, `live`, `fuzz`, `task`, `attacks`, `bench` | holds |
| `hyalos-os/monitor/src/spike.rs:69-86` `spike::inputs`, `:84` its `read_inputs` of the data file | holds |
| `hyalos-os/monitor/src/runner.rs:64-101` `REASON_COUNT` `29 + spike` and `REASONS`, `BadContract` last behind the feature | holds |
| `hyalos-os/audit/src/record.rs:21-56` `Reason`, `BadContract` behind `spike-contract`; `:12` `MAX_EFFECTS` | holds |
| `hyalos-os/audit/src/replay.rs:444` the spike's `BadContract` | holds |
| `hyalos-os/monitor/src/kvm.rs:323` `RECORD_MAX`, 16 MiB (`HEAP` `0x80_0000..0x180_0000`) | holds |
| `hyalos-os/monitor/src/abi_tests.rs:12-20` `PINNED`; `:102-148` `every_reason`; `:393-410` the pin test; `:597-643` `core_1_1_…` | holds |
| `hyalos-os/ABI.md:9-13` the never-edited rule; `:70` the abi/1 input rule | holds |
| `hyalos-os/monitor/src/runner_tests.rs:594` `next() % 29`; `:465-467` the `spike` suffix; `:471-474` the name list | holds |
| `hyalos-os/monitor/src/spike_tests.rs:433` `REASONS.last()`; `:81` `mod with_the_feature` | differs: at `:451` and `:101`, moved by Slice 1's added test; the code is as cited |
| `spike_tests.rs:43`, `ring_tests.rs:63`, `:282`, `spike_evidence_tests.rs:378`, `runner_tests.rs:30`, `confine_tests.rs:241`, `:460`, `profile_tests.rs:18`, `live_tests.rs:157`, `:212`, `:591`, `:660` the fixture reads | holds, within a few lines (Slice 1 moved `ring_tests.rs` and `spike_tests.rs`) |
| `runner_tests.rs:1150`, `:1299`, `confine_tests.rs:501`, `:515`, `live_tests.rs:805`; `tests/audit_cli.rs:236-261`, `tests/release_switches.rs:118-120`, `:188`, `:382`, `:415`, `:536` the fixture passed to the binary | holds |
| `.github/workflows/ci.yml:304` the fuzz step; `hyalos-os/review/README.md:327`; `hyalos-os/attest/probes/two_machines.sh:47`, `:78` | holds |
| `spikes/438/evidence/expected.tsv:11` `micropython-hostile-utf8`, `both` | holds |
| ADR 0138 is the next free number (`docs/adr/` ends at 0137) | holds |

## Acceptance

The executor's host cannot open `/dev/kvm`, and the KVM tests must not run on it: every KVM leg
below skipped here, and a skip is not a pass. The KVM legs are proven by CI (KVM, by CI, below;
Shared contract, KVM steps without `/dev/kvm`).

| ID | How it was shown |
|---|---|
| S-02.1 | Reader side, S-02.1.e1 to e3. The guest side is Slice 3's |
| S-02.1.e1 | `inputs_tests::capsules_s_02_1_e1_a_script_of_several_lines_keeps_every_newline_reader`: a framed script of four newlines comes back whole |
| S-02.1.e2 | `inputs_tests::capsules_s_02_1_e2_an_empty_record_is_kept_reader`: `a`, empty, `c` give three records, and a lone empty record is kept |
| S-02.1.e3 | `inputs_tests::capsules_s_02_1_e3_bytes_that_are_not_utf8_come_back_the_same_reader` |
| S-02.2 | S-02.2.e1 and e2 |
| S-02.2.e1 | `inputs_tests::capsules_s_02_2_e1_a_length_past_the_end_is_refused_before_the_guest_loads`: unit (`ReadError::BadInputs` for a length past the end, a bad magic, a count over 4,096, a length over `RECORD_MAX`, the line form and an empty file; an unreadable file stays `Usage("cannot read inputs")`), then `task-kernel run` on the process backend and with the kernel image named: `refused: BadInputs`, exit 1, no transcript, with a guest file that is not a module, so a loaded guest would have ended `InvalidModule`. KVM leg: CI |
| S-02.2.e2 | `inputs_tests::capsules_s_02_2_e2_stray_bytes_after_the_last_record_are_refused_before_the_guest_loads`: the same, for one byte after the last record |
| S-02.4 | abi/1 side: S-02.4.e2, and `core_s_01_2_e1_the_unchanged_interface_passes` passes with `PINNED` unchanged. The abi/2 section is present and unpinned (2.2) |
| S-02.4.e2 | `abi_tests::capsules_s_02_4_e2_one_changed_word_of_abi_1_fails_the_suite`: `titles as untrusted` changed to `titles as trusted` in memory gives a digest other than the pin; a change inside the abi/2 section leaves the abi/1 digest equal to its pin |
| S-02.6 | S-02.6.e1 and e2 |
| S-02.6.e1 | `inputs_tests::capsules_s_02_6_e1_a_version_4_run_refuses_a_record_holding_a_newline`: a framed record holding a newline, an empty one and one not UTF-8, each beside a good record, give `refused: BadInputs`, exit 1, nothing on standard output, for `run`, `live` (stub), `fuzz`, `task`, `bench` and, in a `test-switches` build, `attacks`; `run` also with the kernel image named (CI). No transcript and no ledger appear |
| S-02.6.e2 | `inputs_tests::capsules_s_02_6_e2_a_version_4_run_of_single_line_records_runs_as_today`: `run` on the fixture framed by `frame --lines` writes byte for byte the transcript `execute` gives the same 40 records in memory, on each backend (KVM: CI) |
| 2.1 | `inputs_tests::capsules_2_1_frame_then_read_gives_back_the_records`: three positional records (several lines, empty, not UTF-8) then a `--lines` file's two non-empty lines read back exactly, and the bytes equal the layout written by hand; a missing file and 4,097 records exit 1 and write nothing; 4,096 records frame |
| 2.2 | `abi_tests::capsules_2_2_abi_md_states_every_hyalos_abi_2_rule`: the section opens with `(draft: not implemented)`, `PINNED` has no `hyalos.abi/2` row, each call row's name, signature and limit, the `read` refusal values, the closed answer values, the grants, the loading, `infer` and `return` refusals in design.md §2.6's order, `ModelRefused`'s type clause, every abi/1 refusal kept and the five new reasons named |
| 2.3 | `inputs_tests::capsules_2_3_bad_contract_takes_byte_29_and_bad_inputs_byte_30`, and `core_s_04_4_the_reason_table_is_closed_and_every_message_round_trips` with its name list ending `… LedgerFailed BadContract BadInputs` in every build; `python_s_01_4_e1_…` asserts 31 |
| 2.4 | `spike_tests::with_the_feature::capsules_2_4_the_spike_split_keeps_its_lines` (`a\nb\n\nc` gives `a`, `b`, `c`, and the issue fixture splits as before); `python_committed_evidence_replays_on_both_backends` passes in the spike build, so every committed spike record replays, `micropython-hostile-utf8` included (process legs here, KVM legs: CI) |
| 2.5 | `docs/adr/0138-hyalos-abi-2-and-record-version-5.md`, written `Proposed`, with its row in `docs/adr/README.md`. Accepted: developer step (Blocked on) |

### Verify, as run by the executor

From the repository root, without `HYALOS_REQUIRE_KVM` (the KVM gates skipped):

- Both `attest` builds: exit 0.
- `cargo test … --features task-kernel/test-trust,hyalos-audit/test-trust,task-kernel/test-switches -- capsules_s_02_ capsules_2_`: `test result: ok. 11 passed`.
- The workspace run with the test features: every binary `test result: ok`.
- The workspace run with the spike-contract features: every binary `test result: ok` (the task-kernel unit tests 285 passed).
- The three clippy runs (default, test features, spike-contract features): exit 0. `cargo fmt --check`: exit 0.
- `cargo xtask layout`: `layout ok: 21 workspace crates`. `cargo xtask lines`: exit 0. `cargo xtask docs`: `docs ok: 194 references in 3 pages`. `cargo xtask review-scope`: `hyalOS review scope: 50 rows agree with 3 lockfiles`. `cargo xtask review-package`: `review package ok: 7 sections, commit under review is an ancestor of HEAD, 31 code references`. `cargo xtask hyalos-unsafe`: `hyalOS: unsafe only in the 11 listed files, each use with a SAFETY comment`. The root `cargo fmt --all --check`: exit 0.
- `grep -c -E -- '-- frame [^ ]+ --lines hyalos-os/evidence/fixtures/issues.tsv'`: `.github/workflows/ci.yml` 1, `hyalos-os/review/README.md` 1. The probe's `"$monitor" frame "$out/issues.inputs" --lines evidence/fixtures/issues.tsv`: 1. `grep -c "(draft: not implemented)" hyalos-os/ABI.md`: 1. The ADR's README row: 1.

### KVM, by CI

To be recorded from the pull request's CI run.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Suggested path 4: a test that expected `ModelRefused` for a title that is not UTF-8 at version 4 now expects `BadInputs` | No test changed | No test passes such a title through a command: the tests that run a guest on a non-UTF-8 record call `execute` in memory, which the run-start rule does not cover (design.md §2.1) | n |
| `spike_tests.rs:433`, `:81` | `:451`, `:101` | Slice 1 added a test above them | n |
| Files: the test fixtures move through `frame --lines` or a helper on the same split | In-memory fixtures use `fixture_lines`, built on the same split `frame --lines` uses (`inputs::lines`); the binary runs frame theirs with `framed_fixture` (unit tests) or the binary's own `frame --lines` (integration tests) | As suggested; noted for the names | n |

## Decisions

- The reader, the `frame` command and the version 4 rule live in a module of their own, `hyalos-os/monitor/src/inputs.rs`, with their tests in `inputs_tests.rs`, so `main.rs` keeps only the callers.
- `frame` also refuses a record over `RECORD_MAX` as a usage error, writing nothing: framing it would break the invariant that `frame` then `read_inputs` gives back the records given.
- `ReadError::BadInputs` displays as `Reason::BadInputs`'s name, so `refused: BadInputs` comes from the reason itself.
- The version 4 rule is `read_v4_inputs`, used by the six default commands; `read_inputs` itself checks only the framing, so Slice 3's `task --version 5` can read a framed file without the rule.
- The usage line is unchanged (`run|replay|task|attacks|bench|probe-host`), since `release_switches.rs` pins it; `frame` is a command beside it, as `forge` and `audit` are.
- `frame` prints `framed: N records` on success, and the probe sends that to `/dev/null`.

## Discoveries

- `live_tests::live_command`'s run directory must stay empty for `core_s_04_5_e1`, so its framed fixture is written to a sibling directory (`<dir>.inputs`).
- `cargo xtask lines` refuses any item after `main.rs`'s first `#[cfg(test)]` other than test modules, so the test helpers `fixture_lines` and `framed_fixture` live in `inputs_tests.rs` and each test file imports them.
- `tests/release_switches.rs` frames the issue fixture once per test binary through a `OnceLock`. Its initialiser must name the line-form fixture itself: calling the framed helper from inside its own initialiser deadlocks the binary.
- The repository workspace (`core/`, `crates/`) is untouched; its tests were left to CI.

## Blocked on

The developer step (Shared contract, developer steps; ADR-032, ADR-035, ADR-036): set
`docs/adr/0138-hyalos-abi-2-and-record-version-5.md`'s Status and its row in `docs/adr/README.md`
to `Accepted` in the pull request's last commit, and set this file's **Status** to `done` in that
commit.
