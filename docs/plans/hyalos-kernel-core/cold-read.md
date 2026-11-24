# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 5 |
| **Result** | Pass after 5 rounds |

## Round 1 — 2026-11-23

The reader thought through Slices 1 to 4 and checked 34 citations: 27 held, 5 partly, and 2 did not. One of those two is the `hyalos.abi/1` name that Slice 1 adds. It found no implementation code. Its tree was a detached worktree of `b3caebc` plus the pack.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q-01 | y | `plan.md` Slice 4, `design.md` Host probe | Where a release build's probe gets its code | `design.md` now says fixed byte sequences in the monitor, placed by the probe's own loader. Slice 8's contract keeps them |
| Q-F1–F4 | n, re-classed | `design.md`, `plan.md` | Mermaid fences | Allowed by `pack-format.md` §6. Not implementation code |
| Q-F5 | n, re-classed | `context.md` §4 | A `bash` fence outside a slice's Verify | It is the template's "How verification works" section, holding only commands |
| Q-02 | n | `prd.md` NFR, Slice 2 | "Cold start" undefined | Adopted the default: boot to first call |
| Q-03 | n | `design.md` start state | `dpl` | The contract already says DPL 3 |
| Q-04 | n | `design.md` start state | Which levels get the User bit | Adopted the default |
| Q-05 | n | `design.md` interface | Refusals not listed | Adopted the default: `ABI.md` lists each `Reason` |
| Q-06 | n | Slice 1 | The kernel image is a separate workspace | Adopted the default: two constants, one test |
| Q-07 | n | Slice 3, `design.md` | Which bits KVM may need | Stop condition and table now agree |
| Q-08 | n | `prd.md`, `design.md` | "Absent" versus zero | Now bits clear and no host data |
| Q-09 | n | `design.md` Host probe | Control not named | A `nop` that must reach the port |
| Q-10 | n | Slice 4 | Which document | `hyalos-os/RECORD.md` |
| Q-11 | n | Slice 4 Verify | Self-test kernel build | The tests build it, citing `profile_tests.rs:51` |
| Q-12 | n | Shared contract | Base commit | The commit the slice branch is cut from |
| Q-13 | n | `prd.md`, Slice 2 | "Committed guests" | `digest.wasm` is the only one, now stated |
| Q-14 | n | `design.md` | §3 missing | Stands: the template deletes Domain model and keeps its numbers |
| Q-15 | n | ADR-004 | Old path | Accepted ADRs are not edited. `context.md` notes the move |
| Q-16 | n | Slice 4 Expected | `RDSEED` measured in 32-bit only | The expectation now defers to the probe |

### Contradictions

| Item | Resolution |
|---|---|
| C-01: ADR-001 versus `context.md` on long-mode evidence | `context.md` notes that the ADR predates the run |
| C-02: 32-bit measurement versus 64-bit probe | `context.md` and Slice 4 state it |
| C-03: Slice 3 stop versus CPUID table | Both changed |
| C-04: "absent" versus zero | `prd.md` and `design.md` changed |
| C-05: ADR-004's old path | `context.md` notes it |
| C-06: ADR-007's line estimate | Stands. It is an estimate, and the budget is the contract |
| C-07: "any committed guest" versus one guest | Shared contract names the one guest |

### Citations checked

| Citation | Holds |
|---|---|
| `hyalos-os/monitor/src/kvm.rs:263`, `:268`, `:144`, `:112`, `:289`, `:181` | yes |
| `hyalos-os/monitor/src/main.rs:958`, `:1057`, `:422`, `:774`, `:549`, `:27` | yes |
| `hyalos-os/monitor/src/main.rs:532` as the expected-reason list | partly, now `:501` |
| `hyalos-os/monitor/src/main.rs:956` | partly, now `:954` |
| `core/hyalos/src/confine.rs:67`, `:99` | yes |
| `hyalos-os/monitor/src/profile_tests.rs:65` | yes |
| `hyalos-os/kernel-image/src/main.rs:261`, `:95`, `:6` | yes |
| `hyalos-os/monitor/src/live.rs:176`, `:202` | yes |
| `crates/xtask/src/main.rs:17`, `:21` | yes |
| `Cargo.toml:14`, `hyalos-os/attest/Cargo.toml:12`, `hyalos-os/Cargo.toml:6` | yes |
| `spikes/task-kernel/ABI.md` in ADR-004 | no, noted in `context.md` |
| `hyalos-os/ABI.md` naming `hyalos.abi/1` | no, Slice 1 adds it |
| `deny.toml:52-54` | partly, now `:50-54` |

## Round 2 — 2026-11-23

The reader thought through Slices 5 to 8 and checked 23 citations: 20 held, 2 partly, and 1 did not. It found no implementation code. Its tree was a detached worktree of `b3caebc` plus the pack.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q-01–Q-04 | n, re-classed | `design.md`, `plan.md` | Mermaid fences | Allowed by `pack-format.md` §6 |
| Q-05 | n, re-classed | `context.md` §4 | `bash` fence outside a slice | The template's verification section, commands only |
| Q-06 | y | Slice 5 | Where the `live` guard sits | In the `live` arm after `load_backend`. `declared_run` is unchanged, and its direct tests keep running |
| Q-07 | y | Slice 8, infra | How the test-only feature works | `test-switches`, the Verify flags, and a `release_switches` test run without it |
| Q-08 | n | Slice 5 | Test for S-04.5.e2 | Both rows are tested through the binary |
| Q-09 | n | Slice 5 | Refusal placement | Before `record_is_fresh`, naming `HYALOS_KERNEL` |
| Q-10 | n | `design.md` §1 | Finding the binary in tests | Adopted the default |
| Q-11 | n | `design.md` channel | Frame encoding | Adopted, with a closed reason table independent of the enum's order |
| Q-12 | n | `design.md` channel | Deadline | Read timeout, kill, `Reason::Timeout` |
| Q-13 | n | `design.md` failure mapping | Pre-chain check | `machine-runner --check` |
| Q-14 | n | Slice 7 | Crates and flags | Pinned in the contract |
| Q-15 | n | `design.md` seccomp | List may grow | Stands: each addition needs a reason |
| Q-16 | n | Slices 7, 8 | When the feature appears | Slice 7 adds it |
| Q-17 | n | `prd.md` S-04.2 | What a test asserts | `SIGSYS`, `KernelFault`, no effect |
| Q-18 | n | Slice 8 | Default-build `attacks` | Exits non-zero, naming the feature |
| Q-19 | n | NFR | Measuring without `HYALOS_TRACE` in release | Measured with `test-switches` |
| Q-20 | n | Slice 8 | Which CI job | The `hyalos-os` job |

### Contradictions

| Item | Resolution |
|---|---|
| C-01: "refused" versus killed by seccomp | `prd.md` S-04.2 and its rows changed |
| C-02: pre-chain refusal versus runner-side failure | `design.md` failure mapping changed |
| C-03: `HYALOS_TRACE` in release | NFR and Slice 8 now agree |
| C-04: guard versus direct `declared_run` tests | Slice 5 contract changed |
| C-05: hostile runner before its gate | Slice 7 adds the feature |
| C-06: Verify lacks the feature | Slices 8, 9 and 12 changed |
| C-07: `RDSEED` expected versus allowed result | Stands: the earlier result was 32-bit, and the probe's measurement governs |

### Citations checked

| Citation | Holds |
|---|---|
| `hyalos-os/monitor/src/main.rs:958`, `:1057`, `:549`, `:774`, `:501` | yes |
| `hyalos-os/monitor/src/kvm.rs:181`, `:112`, `:263`, `:268`, `:41`, `:240` | yes |
| `hyalos-os/monitor/src/profile_tests.rs:65`, `live_tests.rs:430`, `live.rs:176` | yes |
| `hyalos-os/kernel-image/src/main.rs:27`, `hyalos-os/kernel-image/Cargo.toml:14` | yes |
| `Cargo.toml:14`, `crates/xtask/src/main.rs:17`, `core/hyalos/src/confine.rs:67` | yes |
| `docs/adr/0121-no-broker-runs-without-landlock.md`, `hyalos-os/monitor/Cargo.toml:9` | yes |
| `.github/workflows/ci.yml:219` | partly, now `:218` |
| `hyalos-os/monitor/src/main.rs:543` | partly, now `:543–548` |
| `hyalos-os/monitor/src/main.rs:954` | no, now `:956` |

## Round 3 — 2026-11-23

The reader thought through Slices 9 to 12 and checked 17 citations: 14 held and 3 partly. It found no implementation code. Its tree was a detached worktree of `b3caebc` plus the pack.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1–F4 | n, re-classed | `design.md`, `plan.md` | Mermaid fences | Allowed by `pack-format.md` §6 |
| F5 | n, re-classed | `context.md` §4 | `bash` fence outside a slice | The template's verification section, commands only |
| Q1 | y | Slice 9 | No slice created the frame fuzzer, and seed and rounds were unset | Slice 9 creates it and fixes both |
| Q2 | n | Slice 9 | Lint pattern | `unsafe_code = "deny"`, and the pattern changed |
| Q3 | n | Slice 9 | Differential not in Verify | Added as a dry run |
| Q4 | n | Slice 9 | Rounds and seed | In the contract |
| Q5 | n | `design.md` checks | "Non-test" | Defined |
| Q6 | n | `design.md` checks | `unsafe` file list | Seven files listed |
| Q7 | n | `design.md` checks | Detection rule | Defined |
| Q8 | n | Slices 7, 10 | ADR numbers | Stands: the next free number at merge |
| Q9 | n | `design.md` scope | Split positions | `part` field added |
| Q10 | n | Slice 11 | Which crates | Named in the suggested path |
| Q11 | n | `design.md` integration | Advisory matching | Defined |
| Q12 | n | Slice 11 | CI wiring | Added |
| Q13 | n | Slice 12 | Commit under review | The pull request's base commit |
| Q14 | n | Slice 12 | Unfalsifiable grep | Replaced by `review-package` |
| Q15 | n | Slice 12 | Required-sections check | `review-package` |
| Q16 | n | Slice 12 | No-KVM commands | Two command sets |
| Q17 | n | Slice 12 | Issue and row | Named |
| Q18 | n | Slice 12 | Moved limits | Found by their text |

### Contradictions

| Item | Resolution |
|---|---|
| C1: scope's third position versus S-06.1. Blocking | `design.md` changed |
| C2: root `forbid` versus listed `unsafe` files | Slice 9 changed |
| C3: frame fuzzer has no creator | Same as Q1 |
| C4: 9.1 versus Verify | Slice 9 and Shared contract changed |
| C5: Verify could not catch a false review claim. Blocking | `review-package` check |
| C6: Slice 11's dependency | Header and graph changed |
| C7: `HYALOS_REQUIRE_KVM` versus S-07.2.e2. Blocking | Two command sets |
| C8: package cannot name its own commit. Blocking | The base commit |
| C9: ADR-007's starting list | `design.md` lists all seven |
| C10: ADR-007's line figures | `context.md` notes they are an estimate |
| C11: Landlock variable in CI | Stands: Slice 7 adds it |

### Citations checked

| Citation | Holds |
|---|---|
| `crates/xtask/src/main.rs:349`, `:17`, `:430` | yes |
| `hyalos-os/monitor/src/kvm.rs:329`, `profile_tests.rs:65`, `main.rs:924` | yes |
| `.github/workflows/ci.yml:218`, `hyalos-os/attest/Cargo.toml:12`, `hyalos-os/deny.toml` | yes |
| The three `hyalos-os` lockfiles, `coverage.md:45`, `docs/security/`, `docs/adr/README.md`, `hyalos-os/review/` absent | yes |
| `Cargo.toml:14` as a pattern | partly, pattern changed |
| Verifier Slice 7 as #449 | yes |
| Verifier Slice 1 as #443 | partly: the issue is #443, and its pull request has another number |

## Round 4 — 2026-11-23

A second run of the skill. The reader rechecked Slices 9 to 12 and checked 24 citations: 21 held, 2 partly, and 1 did not. It found no implementation code. Its tree was a detached worktree of `b3caebc` plus the pack.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1–Q5 | n, re-classed | `context.md`, `design.md`, `plan.md` | Verification and Mermaid fences | Allowed by `pack-format.md` §6 and the template |
| Q6 | y | Slice 9 Verify | `--all-features` cannot compile `notary` | Lints now run with `test-switches`, and `forging-notary` separately |
| Q7 | y, developer-only | Slice 11, ADR-005 | No position for TLS and certificate crates | Developer answered on 2026-11-23: our use only. ADR-008 records it and amends ADR-005 |
| Q8 | n | Slice 9 | Lints miss two workspaces | Adopted: all three manifests |
| Q9 | n | Slice 9 | Where `unsafe` is allowed | Adopted: a file-level allow |
| Q10 | n | Slice 9 | Frame generator | Left to the executor within the contract's seed and rounds |
| Q11 | n | Slice 9 | #449's state | The default stands: check with `gh` |
| Q12 | n | `design.md` checks | Counter premise | Corrected |
| Q13 | n | Slice 10 | Missing `SAFETY` comments | The default stands |
| Q14 | n | `design.md` | `unsafe` list | The seven files stand |
| Q15 | n | Slice 10 | ADR number | The next free number |
| Q16 | n | `design.md` checks | Unlisted direct dependency | Adopted: it fails the check |
| Q17 | n | `design.md` scope | `tlsn` commit | Adopted |
| Q18 | n | `design.md` integration | Advisory source | Adopted |
| Q19 | n | Slice 11 | Audit values | The default stands: `none found` with a date |
| Q20 | n | Slice 12 | No-KVM commands | Named in the dry run |
| Q21 | n | Slice 12 | Section names | The contract's headings, verbatim |
| Q22 | n | Slice 12 | Required sentence | Same as K5 |
| Q23 | n | Slice 12 | Filing the issue | The default stands |
| Q24 | n | Slice 12 | The named commit | The merge-base |
| Q25 | n | Limits | `attest` test features | Added to the stated limits |

### Contradictions

| Item | Resolution |
|---|---|
| K1: "as the existing counter does". Blocking | `design.md` corrected |
| K2: same as Q6. Blocking | Slice 9 changed |
| K3: lints across workspaces. Blocking | Slice 9 changed |
| K4: two command sets versus Verify. Blocking | Slice 12 changed |
| K5: required wording. Blocking | 12.1 changed |
| K6: same as Q7 | ADR-008 |
| K7: ADR-007's starting list | Stands: "starts with", and `design.md` holds the list |
| K8: `declared_run` citations. Blocking | Now `:152`, `:321` |

### Citations checked

| Citation | Holds |
|---|---|
| `crates/xtask/src/main.rs:17`, `hyalos-os/monitor/src/kvm.rs:263`, `:268`, `:329`, `:112` | yes |
| `hyalos-os/monitor/src/main.rs:1136`, `:924`, `:958`, `:1057` | yes |
| `hyalos-os/monitor/src/profile_tests.rs:65`, `live_tests.rs:430` | yes |
| `Cargo.toml:14`, `hyalos-os/profile/src/lib.rs:8`, `.github/workflows/ci.yml:218` | yes |
| `hyalos-os/attest/Cargo.toml:12`, `hyalos-os/attest/src/bin/notary.rs:679`, `coverage.md:45` | yes |
| `hyalos-os/kernel-image/Cargo.toml:14`, the seven `unsafe` files, `docs/adr/README.md`, the lockfiles, `core/hyalos/src/confine.rs:67` | yes |
| `crates/xtask/src/main.rs:349` as the pattern | partly, premise corrected |
| ADR-007's line counts | partly, labelled an estimate |
| `hyalos-os/monitor/src/live_tests.rs:145`, `:314` | no, now `:152`, `:321` |

## Round 5 — 2026-11-23

The reader thought through Slices 5, 9, 10, 11 and 12, and checked 31 citations: 29 held and 2 partly. It found no implementation code. Its tree was a detached worktree of `63a75ec` plus the pack. One of its searches printed three lines of this file, and it reports not using them.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1–F4 | n, re-classed | `design.md`, `plan.md` | Mermaid fences | Allowed by `pack-format.md` §6 |
| F5 | n, re-classed | `context.md` §4 | `bash` fence outside a slice | The template's verification section, commands only |
| Q6 | n | Slice 5 | "Real repository" | Adopted: any argument other than `stub` |
| Q7 | n | Verify commands | `test-trust` omitted | Adopted: every workspace test command keeps it |
| Q8 | n | Slice 9 | `deny` versus `forbid` | Pattern and reason corrected |
| Q9 | n | Slice 9 | Frame generator | Left to the executor within the contract |
| Q10 | n | Slice 10 | Which counter | Suggested path corrected |
| Q11 | n | `design.md` checks | `unsafe` forms | Adopted |
| Q12 | n | Slice 10 | Where the list lives | The default stands |
| Q13 | n | Slice 10 | ADR number | The next free number |
| Q14 | n | Slice 11 | Which rows | The default stands: every direct dependency |
| Q15 | n | Slice 11 | Daily job | Adopted |
| Q16 | n | Slice 12 | Commit under review | Contract now says what that commit covers |
| Q17 | n | Slice 12 | Issue order | Adopted: file it first |
| Q18 | n | Slice 4 | Exit codes | Corrected |
| Q19 | n | `prd.md` NFR | When the feature applies | Corrected to Slice 8 |
| Q20 | n | `design.md` | §3 missing | Stands: the template keeps its numbers |

### Contradictions

| Item | Resolution |
|---|---|
| C1: ADR-001's evidence | Stands: `context.md` notes it |
| C2: counters | Slice 10 corrected |
| C3: `forbid` pattern | Slice 9 corrected |
| C4: commit under review | Slice 12 contract clarified |
| C5: ADR-007's starting list | Stands: `design.md` holds the list |
| C6: forms of `unsafe` | `design.md` corrected |
| C7: `test-trust` | Verify commands corrected |
| C8: probe exit codes | Slice 4 corrected |
| C9: `RDSEED` on the development host | Stands: the probe's 64-bit measurement governs, recorded as a deviation if it differs |

### Citations checked

| Citation | Holds |
|---|---|
| `hyalos-os/monitor/src/main.rs:327`, `:351`, `:433`, `:150`, `:267`, `:105` | yes |
| `hyalos-os/monitor/src/live_tests.rs:152`, `:321`, `:430`, `live.rs:176`, `profile_tests.rs:65` | yes |
| `hyalos-os/monitor/src/kvm.rs:1`, `:112`, `:181`, `:263`, `:268`, `:329` | yes |
| `crates/xtask/src/main.rs:17`, `:21`, `:349`, `Cargo.toml:14`, `.github/workflows/ci.yml:218` | yes |
| `hyalos-os/audit/src/report.rs:6`, `coverage.md:45`, `core/hyalos/src/confine.rs:67`, `core/hyalos/Cargo.toml:43` | yes |
| `hyalos-os/attest/Cargo.toml:12`, `hyalos-os/kernel-image/Cargo.toml:14`, `hyalos-os/kernel-image/src/main.rs:261` | yes |
| The seven `unsafe` files, the ADR index, and the paths later slices create | yes |
| `hyalos-os/profile/src/lib.rs:8` as a pattern | partly, pattern corrected |
| `Host` in the monitor | partly, now `hyalos-os/audit/src/replay.rs:94` |

## Result

Pass after 5 rounds.
