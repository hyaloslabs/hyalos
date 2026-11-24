# Slice 5 — Live runs require the machine

| | |
|---|---|
| **Status** | done |
| **Issue** | #461 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 |
| **Date** | 2026-11-24 |

## Preflight

Checked on branch `issue/461`, forked from `origin/main` at `5fec709`.

| Citation | Holds |
|---|---|
| `context.md` §1: `HYALOS_KERNEL` chooses the backend, `hyalos-os/monitor/src/main.rs:327` | Yes, by symbol: `load_backend` is now at `:389` |
| `context.md` §2: `live` uses `load_backend` (`:352`) and runs the guest in-process when it is unset | Yes: the `live` arm is now at `:413`, `load_backend` at `:414` |
| Contract: a real repository is any argument other than `stub` (`main.rs:362`) | Yes: the `arg(5)? == "stub"` test is now at `:424` |
| Contract: `record_is_fresh` and the ledger open after `load_backend` | Yes: `:419` and `:438` |
| Contract: `declared_run` called directly with the process backend (`live_tests.rs:152`, `:321`) | Yes, unchanged at both lines |
| `live_tests.rs:430`: tests find the `task-kernel` binary beside the harness | Yes |
| Stop condition: no committed procedure or test runs `live` on a real repository on the process backend | **Differed:** `hyalos-os/attest/probes/two_machines.sh:72` ran `"$monitor" live … "$repo" "$issue"` and never set `HYALOS_KERNEL`. The slice blocked on it (`wip:` commit `a4c496d`), the developer answered (see **Decisions**), and the script now builds the kernel and sets it. Holds now |

No test runs the real `live` command through the binary before this slice (`grep` for `"live"` in `hyalos-os`), and CI's `hyalos-os` job does not run it. `attest/tests/canary.rs` (`probe`, from the `hyalos-kernel-canary` pack) runs `two_machines.sh` with a stub monitor, so it reaches no backend; it needed a stub `cargo` and `kernel-image` directory once the script built the kernel (see **Deviations**).

## Acceptance

| ID | How it was shown |
|---|---|
| S-04.5 | The guard in the `live` arm of `hyalos-os/monitor/src/main.rs`, right after `load_backend` and before `record_is_fresh` and the ledger: on `Backend::Process` with any repository other than `stub`, the command errors with "a live run on a real repository needs the KVM kernel: set HYALOS_KERNEL". `declared_run` is unchanged. The committed operator procedure, `two_machines.sh`, now runs `live` on the kernel it builds: `core_s_04_5_the_probe_script_runs_live_on_the_kernel_it_built` (fails with the `export HYALOS_KERNEL` line removed, counter-checked) and `core_s_04_5_the_probe_script_stops_before_anything_without_a_kernel` in `hyalos-os/attest/tests/canary.rs` |
| S-04.5.e1 | `core_s_04_5_e1_a_real_repository_without_a_kernel_is_refused_with_no_chain_link`: the built binary with `HYALOS_KERNEL` unset and `octo/real` exits non-zero, names `HYALOS_KERNEL`, prints nothing to stdout, creates no `ledger` directory and writes no record, salt or pending file. `core_s_04_5_e1_the_refusal_comes_before_the_record_path_is_checked` shows the order. Both fail with the guard removed (counter-checked). This is also the Verify block's dry run: with `HYALOS_KERNEL` unset, a real repository name exits non-zero and the directory the ledger would go in is unchanged |
| S-04.5.e2 | `core_s_04_5_e2_stub_as_the_repository_runs_without_a_kernel`: `stub` passes the guard and stops at the next check, the record path in use. `core_s_04_5_a_configured_kernel_lets_a_real_repository_through` shows a configured kernel passes it too |

Verify, run after CI's two `attest` `cargo build` steps:

- `cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust core_s_04_5`: 6 passed (4 in `task-kernel`'s `live_tests`, 2 in `attest`'s `canary`), none skipped.
- `HYALOS_REQUIRE_KVM=1 cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust`: 419 passed, 0 failed, and no `skipped:` line, so every KVM test ran on this host's KVM.
- CI's other `hyalos-os` steps: `cargo fmt --check` on `profile`, `audit`, `attest-check` and `verify`; `cargo clippy` on those four crates with `-D warnings`; `cargo deny` on the workspace with all features (advisories, bans, licences and sources ok). Root checks: `cargo xtask layout`, `lines` and `docs` pass. `attest` is not in CI's format list, and `canary.rs` was not rustfmt-clean before this slice, so it was not reformatted (Slice 9).

**Dry run of `two_machines.sh`** (the script is never run for real: it starts a notary and spends authorised attempts). Read against S-04.5's Given, a live run on a real repository:

- `kernel=$PWD/kernel-image/target/x86_64-unknown-none/release/hyalos-kernel` (line 18) and `(cd kernel-image && cargo build --release --locked)` (line 19) build the image from this tree the way the monitor's tests do (`built_kernel` in `monitor/src/profile_tests.rs`: `cargo build --release --locked` in `kernel-image`, whose `.cargo/config.toml` sets `x86_64-unknown-none`). A failed build prints "the kernel image did not build" and exits 2.
- Line 20, the "build first" check, now also requires `[ -s "$kernel" ]`, and says "build both programs and the kernel image first". Lines 19 and 20 come before the first `ssh` (line 24), so a missing kernel stops the script before the canary gate, the notary, the authorisation and the timing call.
- Line 56, `export HYALOS_KERNEL=$kernel`, sets it for the `live` call at line 78, so the monitor takes the KVM backend and the guard lets the run through.
- Run with stub `ssh`, `scp`, `cargo` and programs in a scratch directory: a failing `cargo` exits 2 with no `ssh` or `scp` call; a `cargo` that leaves no image exits 2 at the check with no call; with an image the script passes the check and reaches the first (stub) `ssh`. `bash -n` passes, and `shellcheck` reports only the five info-level notes it reported before the change (SC2015 on the check line, SC2029 three times, SC2153).

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Cited lines `main.rs:327`, `:352`, `:362` | Now `:389`, `:414`, `:424` | Slices 1 to 4 and the verifier pack moved the file | n |
| Test both example rows through the built binary | e2 is shown up to the next check, not to a full run | A full `stub` run opens the ledger, which posts to the public log (`REKOR` is a constant), and no test may reach it. The full `stub` flow on the process backend is `declared_run` with `Mode::Record`, already tested | n |
| Files: `main.rs` and `live_tests.rs` only | Also `hyalos-os/attest/probes/two_machines.sh`, which builds the kernel image, adds it to its "build first" check and exports `HYALOS_KERNEL` for its `live` call | The script was the one committed procedure that ran `live` on a real repository on the process backend (the stop condition), and the developer chose to fix it in this slice | n |
| Files: `main.rs` and `live_tests.rs` only | Also `hyalos-os/attest/tests/canary.rs`: `probe` stubs `cargo` and a `kernel-image` directory, its stub monitor records the `HYALOS_KERNEL` it saw, and two `core_s_04_5_*` tests cover the script | The canary pack's two tests of the script (`canary_s_04_2_e1`, `_e2`) failed once the script built the kernel, since their scratch tree has no `kernel-image`. Their assertions are unchanged | n |

## Decisions

- **Resumed after a block.** The developer (the operator) chose Option 1 of the **Blocked on** question in `a4c496d`: this slice changes `two_machines.sh` to build the kernel image and export `HYALOS_KERNEL`, and adds the kernel to the script's "build both programs first" check. Recorded on #461 (issuecomment-7142854946).
- The script builds the kernel itself rather than only checking for one, as the monitor's tests do, so it never runs a stale image. It does not run `rustup target add`: a missing target fails the build, and the script stops and names it.
- `HYALOS_KERNEL` is exported with the other run settings, and as an absolute path, so the `live` call uses the image built from this tree. Nothing else the script runs reads it: `attest` does not, and the monitor's `task` and `attested` commands do not call `load_backend`.
- The tests give the binary no `HOME`. If the guard were removed, a real-repository run would stop at `no home` before the ledger opens, so a regression fails the test without writing to the public log.
- "No `refused:` string" is read as no chain link with a `refused:` outcome. The command error is still printed as `refused: …` on stderr, as every command error is (`main`).

## Discoveries

- `two_machines.sh` is the only committed caller of `live` with a real repository, and the only script that now builds the kernel image. `attest/tests/canary.rs` runs it with stubs, so a later change to the script's prelude needs a matching stub there.
- The blocked run's preflight missed `canary.rs` because it only searched for the `live` command, not for tests that run the script; the full Verify run found it.
- For Slice 6 (the runner): the script's `live` call now runs on KVM, so once the machine moves into the confined runner the operator's two-machine run exercises it too.
