# Slice 5 — Live runs require the machine

| | |
|---|---|
| **Status** | blocked |
| **Issue** | #461 |
| **PR** | none (no PR is opened) |
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
| Stop condition: no committed procedure or test runs `live` on a real repository on the process backend | **Differs:** `hyalos-os/attest/probes/two_machines.sh:72` runs `"$monitor" live … "$repo" "$issue"` and never sets `HYALOS_KERNEL`. See **Blocked on** |

No test runs the `live` command through the binary before this slice (`grep` for `"live"` in `hyalos-os`), and CI's `hyalos-os` job does not run it.

## Acceptance

Partial: the guard and its tests are written, but the slice is blocked, so the Verify block was not run in full.

| ID | How it was shown |
|---|---|
| S-04.5 | The guard in the `live` arm of `hyalos-os/monitor/src/main.rs`, right after `load_backend`. Tests below |
| S-04.5.e1 | `core_s_04_5_e1_a_real_repository_without_a_kernel_is_refused_with_no_chain_link`: the built binary with `HYALOS_KERNEL` unset and `octo/real` exits non-zero, names `HYALOS_KERNEL`, prints nothing to stdout, creates no `ledger` directory and writes no record, salt or pending file. `core_s_04_5_e1_the_refusal_comes_before_the_record_path_is_checked` shows the order. Both fail with the guard removed (counter-checked) |
| S-04.5.e2 | `core_s_04_5_e2_stub_as_the_repository_runs_without_a_kernel`: `stub` passes the guard and stops at the next check, the record path in use. `core_s_04_5_a_configured_kernel_lets_a_real_repository_through` shows a configured kernel passes it too |

Ran: `cargo test --release --locked --manifest-path hyalos-os/Cargo.toml --workspace --features task-kernel/test-trust,hyalos-audit/test-trust core_s_04_5`, 4 passed, after the two `attest` builds. The whole-workspace run with `HYALOS_REQUIRE_KVM=1` was not run (Blocked path).

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| Cited lines `main.rs:327`, `:352`, `:362` | Now `:389`, `:414`, `:424` | Slices 1 to 4 and the verifier pack moved the file | n |
| Test both example rows through the built binary | e2 is shown up to the next check, not to a full run | A full `stub` run opens the ledger, which posts to the public log (`REKOR` is a constant), and no test may reach it. The full `stub` flow on the process backend is `declared_run` with `Mode::Record`, already tested | n |

## Decisions

- The tests give the binary no `HOME`. If the guard were removed, a real-repository run would stop at `no home` before the ledger opens, so a regression fails the test without writing to the public log.
- "No `refused:` string" is read as no chain link with a `refused:` outcome. The command error is still printed as `refused: …` on stderr, as every command error is (`main`).

## Discoveries

- `two_machines.sh` is the only committed caller of `live` with a real repository.

## Blocked on

**Question:** `hyalos-os/attest/probes/two_machines.sh:72` runs `live` against a real repository and never sets `HYALOS_KERNEL`, so it runs on the process backend, and this slice's guard would refuse it, after the script has started the notary and spent one authorised attempt on the timing call. Should this slice change the script to build the kernel image and export `HYALOS_KERNEL` (adding it to the script's "build both programs first" check), or is the script left for another slice or retired? The script is not in the slice's **Files**.

**Stop condition:** "Stop if a committed procedure or test runs the `live` command against a real repository on the process backend."
