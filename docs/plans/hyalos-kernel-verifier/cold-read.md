# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 15 |
| **Result** | Pass after 3 rounds. |

## Round 1 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 1 and 2 and checked 40 citations: 34 held, 5 held partly, and 1 was reported as not holding but does hold (K9). It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | n | `context.md` §4 | A `bash` fence outside a Verify heading | Re-classed. `context.md` §4 is required to give the exact verify commands, and they are shell commands, not code |
| Q2–Q5 | n | `design.md` §1, §3, §4, §5 | Mermaid fences | Re-classed. The pack format allows `mermaid` for diagrams |
| Q6, Q7 | y | `design.md` §1, `plan.md` Slice 1 | Directory mapping and the evidence set | `design.md` §1 now has the full mapping and defines evidence. Slice 1 points to it |
| Q8–Q10, Q13, Q14 | n | `plan.md` Slice 1 | Path fixes, tag line, `deny` source, ADR number, lockfile versions | Reader's defaults adopted in Slice 1 |
| Q11, Q12 | y | `design.md` infra, `plan.md` Slice 1 | CI trigger and how a skipped job gathers | Both places now state the same set and rule |
| Q15, Q16, Q18, Q19, Q21 | n | `plan.md` Slice 2, `design.md` §2 | Version test location, profile shape, version gates, refusal wording, KVM skip | Reader's defaults adopted |
| Q17 | n | `plan.md` Slice 2 | Local KVM access | The pack stands: CI's KVM run is the proof, and a host without KVM reports a skip |
| Q20 | y | `design.md` infra, `plan.md` Slice 3 | The stand-in variable list | All five are named. The reader's default would have broken `attest`'s release-build tests, so only code `hyalos-verify` links is gated |
| Q22 | y | `design.md` §2, `prd.md` assumptions | Exit code for an over-limit log reply or list | Inconclusive, exit 2 |
| Q23 | y | `design.md` §2 | Temporary `--verify` | Added to the command contract for Slice 3 only |
| Q24, Q25 | y | `plan.md` Slices 5 and 6 | Operator-only proof could not fit a `done` slice | S-05.1 to S-05.3 are split: workflow side in Slice 5, real-run side in Slice 6, because GitHub runs a manual workflow only from `main` |
| Q26 | n | `prd.md` S-05.6.e1 | `profiles` line | Added to the PRD row |
| Q27 | y | `plan.md` Slice 3 Verify | No command for 3.2 | Added |

### Contradictions

| Item | Resolution |
|---|---|
| K1: command contract and temporary `--verify` | `design.md` §2 changed |
| K2, K3: move destinations | `design.md` §1, `plan.md` Slice 1 and `prd.md` S-01.2.e1 changed |
| K4: test tag and the no-tag rule | `design.md` §6 changed |
| K5: an over-limit list as a refusal | `design.md` §2 and `prd.md` assumptions changed |
| K6: CI trigger set | `design.md` infra changed |
| K7: ADR-005's text and ADR-008 | No change. ADR-005 is accepted and cannot be edited, and its Status line names ADR-008 |
| K8: `profiles` line | `prd.md` changed |
| K9: 302 tracked files | No change. `spikes/task-kernel` has 302, and the reader's 333 counted all of `spikes/` |
| K10: 141 attest tests against 143 attributes | No change. 141 is the measured run, and `attest` has feature-gated test code |
| K11: blocked rows in a finished slice | Resolved with Q24 |

### Citations checked

| Citation | Holds |
|---|---|
| Workspace, toolchain, line budget and layout rules (`Cargo.toml:14`, `rust-toolchain.toml:2`, `crates/xtask/src/main.rs:17`, `:274`) | yes |
| CI, CODEOWNERS, `CLAUDE.md`, `deny.toml` and release workflow lines | yes |
| Monitor limits, `wasmi` settings, `task_v2`, audit options and stand-in log | yes |
| Ledger log URL, key, `openssl` wrapper and link version range | yes |
| `attest` release identifier, tag lookup and notary signer | yes |
| `kernel/tests/audit_cli.rs:41`, `kernel/src/main.rs:1087`, `attest/src/lib.rs:85` | partly: each is within a line or two, or breaks only after the move, as the pack says |
| 171 presentations, largest record, 137 and 7 monitor tests | yes |
| 141 `attest` tests | partly (K10) |
| 302 tracked files | yes, after re-counting (K9) |

## Round 2 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3 and 4 and checked 24 citations: 18 held, 5 held partly, and 1 could not be checked because it is in the TLSNotary source outside the repository. It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1–Q5 | n | `design.md`, `context.md` §4 | Mermaid fences and the verify commands | Re-classed, as in round 1. The pack format allows both |
| Q6 | y | `plan.md` Slice 3 | What the forgery corpus is and how it runs | Slice 3 defines it and where its harness lives |
| Q7 | y | `plan.md` Slices 1, 3, 4 | Whether the monitor's `audit` also loses `--verify` | It does, in Slice 4. With one library, an accepted but ignored checker would misstate what ran |
| Q8 | y (re-classed) | `plan.md` Slice 3 | The verifier forwards provider trust variables to the child `attest verify` | The reader's default left a forged-provider path open only if unbuilt, so it is now mandatory: a build without `test-trust` launches the child with those variables removed, under a new stop condition |
| Q9–Q16 | n | `plan.md` Slices 3 and 4 | Golden set, error mapping, host trait, local IDs, redirects, field comparison, trust type, three `openssl` sites | Reader's defaults adopted |
| Q17 | y | `plan.md` Slices 4 and 6 | Where the real empty-`PATH` run is proven | Slice 4 uses a test build with the stand-in log, and Slice 6 proves it on the real log |

### Contradictions

| Item | Resolution |
|---|---|
| C1: refusal wording for an unknown option | `prd.md` S-04.4 changed to `rejected`, matching exit 2 |
| C2: where the provider trust read lives | `design.md` infra changed: `hyalos-audit` reads under `test-trust`, and `attest-check` takes a value |
| C3: monitor's options unchanged, and `--verify` removed | `plan.md` Slices 1, 3 and 4 changed |
| C4: forwarded variables reach the child | Resolved with Q8 |
| C5: a separate record crate | `context.md` changed: the types move to `hyalos-audit` |
| C6: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |
| C7: real record and stand-in log | Resolved with Q17 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor limits, `openssl` helper, log URL and key, link range, `Scratch`, `Auditor` | yes |
| `attest` `verify`, trust read and notary signer | yes |
| CI classification, line budget, layout rule, repository ADRs, 171 presentations, 302 tracked files | yes |
| `task_v2`, `live_tests.rs:21`, `ci.yml:165`, `attest/build.rs:5`, `CLAUDE.md` variable list | partly: each is within a line or two, or is a later slice's change |
| TLSNotary `crates/core/Cargo.toml:52`, `crates/attestation/src/signing.rs:260` | not checkable in the reader's tree. The author opened both at the pinned tag |

## Round 3 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 5 and 6 and checked 27 citations: 22 held, 2 held partly, 2 are in the TLSNotary source outside the repository, and 1 did not hold (the version of `out/live3.json`). It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1, F2 | n | `context.md` §4, `design.md` | The verify commands and mermaid fences | Re-classed, as in rounds 1 and 2. The pack format allows both |
| Q1 | y | `prd.md` S-02.3.e1, `design.md` §6, `plan.md` Slice 6 | The real version 3 record was not identified, and `out/live3.json` is version 1 | Named: `out/two-machines-canary/` is the only committed version 3 record. S-02.3.e1 now names one record of each version. Author confirmed the versions on disk |
| Q2 | y | `context.md` §2, `plan.md` Slice 5 | `release.yml`'s `v*` filter also matches `verifier-v*` | Slice 5 narrows it to `v[0-9]*`, with a `release_gate` assertion |
| Q3–Q6, Q8–Q10, Q18 | n | `plan.md` Slice 5 | Evidence list, builder, commit source, double build, manual mode, tag rule, gate condition, hash test | Reader's defaults adopted |
| Q7 | y (with C3) | `plan.md` Slice 5 | No file holds the provenance command | The new ADR holds it, and the guide repeats it |
| Q11, Q13, Q14, Q16, Q17 | n | `plan.md` Slice 6 | Container, release names, key sources, limits test, conditional limits | Reader's defaults adopted |
| Q12 | y (with C2) | `plan.md` Slice 6 | Whether Slice 6 can start before the operator's run | It can. Only the release-side rows wait |
| Q15 | y (with C5) | `plan.md` Slice 6 | Network test could skip and still look green | `HYALOS_REQUIRE_PUBLIC_LOG` is set in Verify |

### Contradictions

| Item | Resolution |
|---|---|
| C1: `release.yml` unchanged, and its filter matches verifier tags | Resolved with Q2 |
| C2: Slice 6 stop and dry runs | Resolved with Q12 |
| C3: Slice 5 must write a command with no file for it | Resolved with Q7 |
| C4: stop on attestations, and an operator attestation | `plan.md` Slice 5 now says the stop binds agents |
| C5: Verify could pass with S-06.1 skipped | Resolved with Q15 |
| C6: `out/live3.json` called version 3 | Resolved with Q1 |
| C7: pinned compiler and an unpinned `musl-tools` | Slice 5 pins the package version |
| C8: "or" against "both" | `plan.md` Slice 5 Outcome changed |
| C9: evidence script not in Files | Added to Slice 5 |
| C10: README says nothing is open | README now lists the operator's steps |
| C11: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, identity and report lines, ledger constants and link range | yes |
| `attest` trust read and notary signer, line budget, toolchain, CI, CODEOWNERS and release lines | yes |
| Forgery tests, 171 presentations, largest record | yes |
| `release.yml:36` and `ledger.rs:370` | partly: the first names the musl C build and not `ring` by name, and the second is one line off |
| TLSNotary `crates/core/Cargo.toml:52`, `crates/attestation/src/signing.rs:260` | not checkable in the reader's tree. The author opened both at the pinned tag |
| `out/live3.json` as a version 3 record | no. Fixed (Q1) |

## Round 4 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 5 and 6 and checked 35 citations: 30 held and 5 held partly. It flagged the pack's allowed fences as code, and there is no implementation code. The author then ran today's audit on the version 3 canary record with its full chain and a stand-in log of the stored replies, offline: it exited 0 with a 20-line report.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Fences | n | `context.md` §4, `design.md` | The verify commands and mermaid fences | Re-classed, as in earlier rounds. The pack format allows both |
| Q1 | y | `design.md` §2, `prd.md` S-06.1, `plan.md` Slice 6 | The command took a record directory, but the audit takes a record file and a separate chain directory | The contract now takes `<record-file> <links-dir>`, with the operator key required and the inputs a clean container needs listed |
| Q2 | n | `plan.md` Slice 6 | Whether the canary's links are in the public log | The pack stands: Slice 6 checks it first and has a stop condition |
| Q3 | y (with X1) | `design.md` §2 | The manual mode's `publish` input | Removed. Manual mode never creates a release |
| Q4–Q9, Q15 | n | `plan.md` Slice 5, `design.md` §2 | Commit source, `musl-tools` pin, evidence argument, ancestor check, gate coverage, path remapping, attestation command | Reader's defaults adopted |
| Q10–Q12, Q14 | n | `plan.md` Slice 6 | Variable list, test location, recording the operator's run, TLS roots | Reader's defaults adopted |
| Q13 | y (with X4) | `design.md` infra, `plan.md` Slice 3 | Whether the monitor's release build reads the stand-in variables | ADR-004 governs: the monitor's reads are gated too. Only `attest`'s own programs keep theirs |

### Contradictions

| Item | Resolution |
|---|---|
| X1: manual `publish` input against tag-only publishing | Resolved with Q3 |
| X2: record directory against record file and chain | Resolved with Q1 |
| X3: no slice adds `HYALOS_REQUIRE_PUBLIC_LOG` to `CLAUDE.md` | Slice 6 adds it |
| X4: ADR-004 against the monitor reading stand-in variables | Resolved with Q13 |
| X5: `musl-tools` pin not in the plan's steps | Added to Slice 5 |
| X6: ADR-006 cites `release.yml:36` for `ring` | No change, because ADR-006 is accepted. `design.md` and `context.md` cite `build-release-binary.sh` for the build |
| X7: how the ancestor check is enforced | Added to Slice 5 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor limits, settings, identity, report, ledger constants and link range | yes |
| `attest` imports, trust read, notary signer and `forge-ticket` | yes |
| CI, CODEOWNERS, release, evidence and build scripts, `release_gate`, repository ADRs | yes |
| `attest/build.rs:5`, `release.yml:195`, `release.yml:36`, `ci.yml:165`, `CLAUDE.md:44` | partly: each is within a line or two, or is a later slice's change |
| Canary record is version 3, `live3.json` is version 1 | yes |

## Round 5 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3, 5 and 6 and checked 33 citations: 28 held, 3 held partly, 1 was a real gap (the `CLAUDE.md` list), and 1 could not be checked from its tree (the commit). It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` Slice 6 | Whether the operator's key comes from a separate channel or from the chain | A customer's key comes by a separate channel. The test's key is derived from the stored entry only because none is committed, so it proves mechanics and not where a key came from |
| Q2–Q6 | n | `design.md`, `context.md` §4 | Mermaid fences and the verify commands | Re-classed, as in earlier rounds. The pack format allows both |
| Q7 | y (with C2) | `plan.md` Slice 3 | Golden set narrower than S-03.1 | Now every committed record the audit passes, plus the test transcripts |
| Q8 | n | `context.md` §2 | Unassigned `ledger.rs` lines | Each range now has a fate. Operator key and signing code stays in the monitor |
| Q9–Q15, Q17–Q20 | n | `plan.md` Slices 3, 5, 6, `design.md` §2 | Harness, release-build test, error rule, salt and count limits, time limit, build flags, CI name, key format, container files, match rule, skip rule | Reader's defaults adopted |
| Q16 | n | `plan.md` Slice 6 | Whether the canary is in the live log | The pack stands: Slice 6 checks it first and has a stop condition |

### Contradictions

| Item | Resolution |
|---|---|
| C1: the operator key's source | Resolved with Q1 |
| C2: golden set | Resolved with Q7 |
| C3: `<links-dir>` default | `plan.md` 3.1 says the monitor keeps its default and the verifier has none |
| C4: `CLAUDE.md`'s list | Slices 2 and 6 now add the variables, and Slice 2's Files name the file |
| C5: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, transcript, stand-in log, `task_v2`, limits, link range and log constants | yes |
| `attest` trust read and `verify`, forgery test files, 171 presentations, canary record | yes |
| Release, evidence and build scripts, `release_gate`, CI lines, repository ADRs | yes |
| `ledger.rs:370`, `main.rs:1087`, `release.yml:36` | partly |
| `CLAUDE.md` `HYALOS_REQUIRE_*` list | no. Fixed (C4) |

## Round 6 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 2, 3 and 6 and checked 35 citations: 29 held, 5 held partly, and 1 is in the TLSNotary source outside the repository. It flagged the pack's allowed fences as code, and there is no implementation code. The author confirmed that the committed `out/two-machines-slice5/operator.pub` equals the key on all 12 links.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1–Q5 | n | `design.md`, `context.md` §4 | Mermaid fences and the verify commands | Re-classed, as in earlier rounds. The pack format allows both |
| Q6 | y | `plan.md` Slice 3 | The forgery corpus builds records through the monitor's live path | The monitor keeps those tests and generates fixtures. The verifier's tests run only the built binary on them |
| Q13 | y | `plan.md` Slices 5 and 6 | The model provenance command cannot pass for the new workflow or a manual run | The Slice 5 ADR holds both forms: the tag ref for a release, and `refs/heads/main` for the manual run |
| Q14 | y | `plan.md` Slice 6 | The real-run key came from the chain itself | Slice 6 uses the committed `out/two-machines-slice5/operator.pub`, a file apart from the chain |
| Q15 | y | `plan.md` Slice 3 | The Brief's unchecked provider proof had no acceptance | Local criterion 3.9 holds the existing report line |
| Q7, Q8 | n | `plan.md` Slice 3 | Error mapping and golden commands | Reader's defaults fit the pack. The progress file lists them |
| Q9–Q12 | n | `plan.md` Slice 2 | Optional field bytes, KVM runner, crate features, remaining version gates | Reader's defaults adopted. CI uses the runner the microVM job already uses |
| Q16–Q18, Q20 | n | `plan.md` Slices 4 to 6 | TLS roots, conditional-limit marking, default trust, `musl-tools` source | Reader's defaults fit the pack, and the pin's source is now stated |
| Q19 | n | `prd.md` non-functional requirements | Time and size targets have no check | Local criterion 4.6 records both |

### Contradictions

| Item | Resolution |
|---|---|
| K1: release builds and `--release --workspace` test runs | The shared contract now exempts test runs and fixes how the checked binary is built |
| K2: the model provenance command | Resolved with Q13 |
| K3: usage errors share exit 2 with outages | No change. Neither is a pass, and S-04.4.e1 was approved with exit 2 |
| K4: the real-run key | Resolved with Q14 |
| K5: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |
| K6: a recorded step in place of a test | 3.8 now requires tests, and S-03.7's release-build check is a test |
| K7: where the `musl-tools` pin comes from | `plan.md` 5.6 changed |
| K8: the corpus and the live path | Resolved with Q6 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor transcript, replay, identity, limits, stand-in log and defaults | yes |
| Ledger constants, link range and `openssl` helper | yes |
| `attest` imports, trust read, signer, tag lookup and paths | yes |
| Release filter, build script, CI budget, ADR-0111, record versions and sizes | yes |
| `main.rs:1087`, `main.rs:587`, `release.yml:207`, `ci.yml:165`, `releases.md:21` | partly |

## Round 7 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3 and 4 and checked 36 citations: 33 held, 2 held partly, and 1 pair is in the TLSNotary source outside the repository. It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` Slice 4 | If `attest verify` calls `attest-check`, S-04.2 compares the check with itself | `attest verify` keeps its own full-`tlsn` check as the independent judge, and `attest-check` is used only by the audit. The comparison lives in `hyalos-os/verify/tests/` |
| Q2–Q6 | n | `design.md`, `context.md` §4 | Mermaid fences and the verify commands | Re-classed, as in earlier rounds. The pack format allows both |
| Q7 | n | `plan.md` Slice 4 Verify | The dependency check and S-03.5 are not run | Reader's default adopted |
| Q8, Q9, Q11, Q12, Q15, Q16 | n | `plan.md` Slices 3 and 4 | Golden inputs, typed verdict, child variables, generator's KVM need, meaning of altered, `--verify` removal | Reader's defaults fit the pack |
| Q10 | y (with K5) | `plan.md` 3.1 | Whether the limits apply to the monitor's audit | They do, because there is one audit. 3.1 now says its behaviour is unchanged for inputs within the limits |
| Q13 | n | `plan.md` 4.1 | The root-store feature | Reader's default adopted |
| Q14 | n | `plan.md` 4.2 | Redirects on the shared log agent | Reader's default adopted |
| Q17 | n | `plan.md` 3.9 | Test name for a local criterion | Reader's default adopted |

### Contradictions

| Item | Resolution |
|---|---|
| K1: the check and its oracle | Resolved with Q1 |
| K2: "the only library that reads them" | `design.md` now says the only library on the audit path |
| K3: requirements called targets | `plan.md` 4.6 now fails the slice on a miss |
| K4: three variables against five | `design.md` now names the three provider variables |
| K5: limits and the monitor's unchanged behaviour | Resolved with Q10 |
| K6: which job owns S-02.5 | `design.md` R3 and `prd.md` S-02.5.e2 now name the `hyalos-os` job |
| K7: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, identity, engine, limits and report lines | yes |
| Ledger `openssl` sites, log agent and link checks | yes |
| `attest` imports, trust read, `verify`, signer and features | yes |
| Layout, CI, release filter, ADRs, lockfile versions, record versions and sizes | yes |
| `main.rs:1087`, `build-release-binary.sh:8,15`, `releases.md:21` | partly |

## Round 8 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3 and 4 and checked 32 citations: 28 held, 3 held partly, and 1 did not hold because it described a Slice 4 change as present (C1). It flagged the pack's allowed fences as code, and there is no implementation code. It ran one command on `cold-read.md` with the output discarded, and reports reading none of it.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1–Q5 | n | `design.md`, `context.md` §4 | Mermaid fences and the verify commands | Re-classed, as in earlier rounds. The pack format allows both |
| Q11 | y (with C6) | `plan.md` 4.6 | No committed record has 5 answers | The Slice 3 fixture generator makes one against the stand-in provider, and 4.6 times it |
| Q12 | y | `plan.md` 4.4 | The trust inputs for the 171 presentations, without which agreement could be vacuous | 4.4 names them, and requires every canary presentation to be accepted by both judges |
| Q6, Q7, Q10, Q14–Q16 | n | `plan.md` Slices 3 and 4 | Golden inputs, typed verdict, stand-in test, reply cap, root feature, altered copies | Reader's defaults fit the pack |
| Q8 | n | `plan.md` 3.6 | Fixtures need fixed keys to be comparable | Reader's default adopted |
| Q9 | n (with C7) | `plan.md` 3.9 | A local criterion named like a PRD one | 3.9 now says it is local, and its test is `local_3_9_...` |
| Q13, Q18 | n | `plan.md` 4.4, 4.6 | Fields compared, and what counts as the verifier's lines | Reader's defaults adopted |
| Q17 | y (with C2) | `plan.md` Slice 4 | Which programs lose `--verify` | The monitor's audit and `hyalos-verify`. `attest` is unchanged |

### Contradictions

| Item | Resolution |
|---|---|
| C1: the log limits read as present behaviour | `design.md` now says Slice 4 adds them |
| C2: `--verify` and `attest` | Resolved with Q17 |
| C3: `attest` must not hold a second check | `design.md` §1 now states the kept judge, and that the audit never calls it after Slice 4 |
| C4: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |
| C5: ADR-004 and the child until Slice 4 | No change. `plan.md` 3.5 clears the child's environment until Slice 4 removes it |
| C6: no 5-answer record | Resolved with Q11 |
| C7: `s_03_9` | Resolved with Q9 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, transcript, identity, limits and report lines | yes |
| Ledger `openssl` sites, link range and refusal comment | yes |
| `attest` imports, trust read, `verify`, signer and features | yes |
| Layout, CI, deny, build script, corpus and presentation count | yes |
| `ledger.rs:22` and `:25`, `release.yml:195` and `:207`, canary answer count | partly |
| `ledger.rs:248` as already refusing redirects | no. Fixed (C1) |

## Round 9 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3 and 4 and checked 30 citations: 29 held and 1 held partly. It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` 3.6 | Fixtures compared byte for byte cannot match, because ECDSA signatures differ on each run | Drift is now checked by verdict and named check. The generator uses throwaway keys, so no private key is committed |
| Q2–Q6 | n | `design.md`, `context.md` §4 | Mermaid fences and the verify commands | Re-classed, as in earlier rounds. The pack format allows both |
| Q7 | y (with K1) | `plan.md` Slices 3 and 4 Verify | The monitor's `test-trust` feature is never enabled | Every test command now enables `task-kernel/test-trust` and `hyalos-audit/test-trust` |
| Q8 | n | `plan.md` 3.7 | Where golden reports live | Reader's default adopted |
| Q9, Q10, Q13–Q16 | n | `plan.md` Slices 3 and 4 | Error table, monitor limits, root feature, redirects, signature set, output shape | Reader's defaults fit the pack |
| Q11 | n | `plan.md` 3.6 | Committed test private keys and the secret scanner | No longer applies. No private key is committed |
| Q12 | n | `plan.md` 3.9 | A local criterion is not in the traceability table | No change. The table lists PRD criteria only |
| Q17–Q19 | n | `plan.md` 4.3, 4.4 | `out/q.presentation`, altered copies, a refused canary | Reader's defaults adopted |
| Q20 | n (with K2) | `plan.md` 4.6 | Scope of the line count | No change. Counting all three crates is stricter than the PRD |

### Contradictions

| Item | Resolution |
|---|---|
| K1: the monitor's feature and the Verify commands | Resolved with Q7 |
| K2: the line limit's scope | Resolved with Q20 |
| K3: altered copies | `plan.md` 4.3 and 4.4 now name them |
| K4: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor limits, engine, report, identity, audit inputs and stand-in log | yes |
| Ledger `openssl` sites, key reading, link checks and log constants | yes |
| `attest` trust read, `verify`, features, build script and test paths | yes |
| Toolchain, CI, release, build script, ADRs, CODEOWNERS and `CLAUDE.md` | yes |
| `ledger.rs:248` as the place redirects get set | partly: Slice 4 adds the setting |

## Round 10 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack, after the re-route to size L and the re-slice to seven slices. It thought through Slices 3 and 4 and checked 25 citations: 21 held and 4 held partly. It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1–Q6 | n | `context.md` §4, `design.md`, `plan.md` | The verify commands and mermaid fences | Re-classed, as in earlier rounds. The pack format allows both |
| Q8 | y (with C3), re-classed by the author | `design.md` §2 | When a missing `--guest` is detected. The reader's default, exit 2 before any file is read, would break version 1 records, which need no guest (`main.rs:847`) | `design.md` now checks it after reading the record's version. A later record without it exits 2 |
| Q7 | n | `plan.md` 3.6 | Golden set inputs and names | Reader's default adopted |
| Q9 | n (with C2) | `design.md` §2 | Which slice adds the 1 MiB reply cap | Slice 5, as `plan.md` 5.2 says. The limits table now says so, so the reader's default of Slice 3 is not adopted |
| Q10 | n | `plan.md` Slice 3 | Local 3.7 and S-03.7 share a number | No change. Their test names differ |
| Q11 | n | `plan.md` 3.5 | Provider variables in a `test-trust` build | Reader's default adopted |
| Q12 | n | `design.md` §2 | The replay host's signature | Reader's default fits the pack |
| Q13–Q17 | n | `plan.md` Slice 4 | Manifest and expected files, what counts as the corpus, pass fixtures, which build, where programs are found | Reader's defaults adopted |

### Contradictions

| Item | Resolution |
|---|---|
| C1: the README names Slice 4 for the `tlsn` build | README now says Slice 5 |
| C2: the reply cap's slice | Resolved with Q9 |
| C3: exit codes for missing options | Resolved with Q8. `--legacy-v1` is optional, and its absence on a version 1 record is a refusal |
| C4: the forging notary's path | `plan.md` Slice 4 pattern now says it resolves from the shared target |
| C5: 3.7 and the corpus | No change. 3.7 is a local test outside the S-03.2 manifest |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, transcript, identity, inputs, stand-in log and report lines | yes |
| Ledger link range, log constants, key reading and agent | yes |
| `attest` trust read, `verify` and features, and repository layout, budget and release lines | yes |
| `live_tests.rs:21`, `attest/tests/common/mod.rs:169`, and the post-move `audit_cli.rs` and corpus paths | partly |

## Round 11 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3, 4 and 5 and checked 42 citations: 39 held and 3 held partly. It flagged the context verify commands as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1, Q2 | n | `context.md` §4, `design.md`, `plan.md` | The verify commands and mermaid fences | Re-classed, as in earlier rounds. The pack format allows both |
| Q3 | y | `design.md` §2 | Whether a gap in the evidence is refused or inconclusive | A file named on the command line that cannot be read is inconclusive, matching S-03.3.e2. A missing salt, presentation or link is refused, because the operator supplies them. The audit already fails on each (`main.rs:857`, `:678`, `ledger.rs:117`) |
| Q4 | n (with K2), re-classed by the author | `plan.md` 5.4 | Which trust applies to each presentation | 5.4 already gives one rule with one stated exception. It now also says a stand-in provider's presentation counts as refused by both |
| Q5 | n (with K5) | `plan.md` 4.4 | The unchecked-proofs line needs a ticketed record | Reader's default adopted |
| Q6 | n (with K1) | `plan.md` 4.1 | Which build proves S-03.2 | Reader's default adopted |
| Q7 | n (with K4) | `plan.md` 3.6 | What golden covers | See K4 |
| Q8, Q9, Q11–Q13, Q16 | n | `design.md`, `plan.md` | Trait shape, link reads, redirects, test filters, operator key, conditional lines | Reader's defaults fit the pack |
| Q10 | n (with K3) | `plan.md` 5.6 | Line-count scope | No change. Counting all three crates is stricter than the PRD |
| Q14 | n (with K6) | ADR-005 | ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |
| Q15 | n (with K7) | `README.md` | ADR-006 cited for the `tlsn` choice | README now cites `plan.md` 5.1 |

### Contradictions

| Item | Resolution |
|---|---|
| K1: S-03.2 and the release build | Resolved with Q6. S-03.7 covers the release build |
| K2: two trust clauses | Resolved with Q4 |
| K3: the line limit's scope | Resolved with Q10 |
| K4 (blocking): the golden set included refused transcripts, and a refusal has no report | `plan.md` 3.6 now holds passes only. Refusals stay in the monitor's tests and Slice 4 |
| K5: the conditional unchecked-proofs line | Resolved with Q5 |
| K6: ADR-005 and ADR-008 | Resolved with Q14 |
| K7: the README's citation | Resolved with Q15 |
| K8: the audit's usage text brackets `--operator-public` | No change. The design requires it, as the code does |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, transcript, identity, engine, limits, stand-in log and report lines | yes |
| Ledger constants, `openssl` sites, key reading and link checks | yes |
| `attest` imports, trust read, `verify`, signer, build script and lockfile versions | yes |
| Layout, CI, deny, CODEOWNERS, `CLAUDE.md`, ADRs, record sizes and presentation counts | yes |
| `ledger.rs:248`, `release.yml:207`, TLSNotary's upstream lines | partly |

## Round 12 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3, 4 and 5 and checked 34 citations: 31 held, 2 held partly, and 1 pair is in the TLSNotary source outside the repository. It flagged the pack's allowed fences as code, and there is no implementation code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y (with X1) | `plan.md` 3.6 | `audit_tests.rs` makes its keys at run time, so the golden set had nothing fixed to capture, and 3.6 read as storing keys | Each is captured once at the base commit. Transcript, public keys, stand-in log and report are committed, and no private key is |
| F-1 to F-6 | n | `context.md` §4, `design.md`, `plan.md` | The verify commands and mermaid fences | Re-classed, as in earlier rounds. The pack format allows both |
| Q2, Q4, Q10, Q11 | n | `plan.md` Slices 3 and 5 | Error table, test names, redirect setting, line count | Reader's defaults fit the pack. 5.2 now says the setting is added |
| Q3 | n (with X8) | `design.md` §2 | An existing salt that cannot be read | Reader's default adopted |
| Q5 | n (with X2) | `plan.md` 4.1 | What a case is, and what checks the manifest | Reader's default adopted |
| Q6 | n (with X3) | `plan.md` 4.1 | The corpus runs on the `test-trust` build | No change. S-03.7 proves the release build ignores stand-ins |
| Q7 | n | `plan.md` 5.6 | Which build is timed | Reader's default adopted |
| Q8 | n | `plan.md` 5.1 | Where `attest-check`'s default trust comes from | Reader's default adopted |
| Q9 | n | `plan.md` 5.4 | Presentations with no committed notary key | Reader's default adopted. They are counted separately |
| Q12 | n | `plan.md` 3.6 | Which keys each committed record uses | Reader's default adopted |

### Contradictions

| Item | Resolution |
|---|---|
| X1: stored keys and public keys only | Resolved with Q1 |
| X2: tests and cases | Resolved with Q5 |
| X3: S-03.2 and the shipped build | Resolved with Q6 |
| X4: `--guest` bracketed in the form | No change. The Inputs row states when it is required |
| X5: the line limit's scope | No change. Counting all three crates is stricter than the PRD |
| X6: redirects read as existing | `plan.md` 5.2 changed |
| X7: `local_3_7` and `s_03_7` | No change. They prove different items |
| X8: an unreadable salt | Resolved with Q3 |
| X9: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, transcript, identity, stand-in log and report lines | yes |
| Ledger constants, `openssl` sites, link checks and refusal comment | yes |
| `attest` trust read, `verify` and signer | yes |
| Layout, CI, release, build script, ADRs, evidence counts, versions and sizes | yes |
| `main.rs:1087`, `ledger.rs:248` | partly |
| TLSNotary's upstream lines | not checkable in the reader's tree |

## Round 13 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3, 6 and 7 and checked 32 citations: 27 held, 4 held partly, and 1 was reported as not holding but does hold (K4). It flagged the pack's allowed fences as code, and there is no implementation code. The author then ran the built `attest verify` with an empty environment on all 40 canary presentations: all 40 were accepted under the default real trust, naming `api.anthropic.com`.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q01–Q03 | n | `context.md` §4, `design.md`, `plan.md` | The verify commands and mermaid fences | Re-classed, as in earlier rounds. The pack format allows both |
| Q17 | y | `plan.md` 7.2 | Whether the canary's presentations come from the real provider | Checked by the author, and 7.2 now records the result |
| Q16 | n, re-classed by the author | `plan.md` 7.2 | Whether the public log lists exactly this chain | Cannot be settled offline. `plan.md` Slice 7, Stop conditions, already stops if no committed real record passes against the public log |
| Q04, Q05 | n (with K2) | `plan.md` 3.6 | Which commit, and how run-time records are captured | Reader's defaults adopted |
| Q06, Q10, Q11, Q15, Q19–Q21, Q23 | n | `plan.md`, `design.md` | Error table, `musl-tools` lookup, target, evidence job, audit rerun, roots, guide format, ADR numbers | Reader's defaults fit the pack |
| Q07 | n | `plan.md` 3.4 | A `--verify` program that cannot launch | Reader's default adopted |
| Q08 | n | `context.md` §2 | `HIDDEN_BYTES_LIMIT` | Reader's default adopted |
| Q09 | n | `plan.md` 3.1 | Which limits reach the monitor. The reader's default is right for file limits and wrong for log limits | 3.1 now says file limits are the verifier's and log limits are shared. `design.md` §1 and `plan.md` 5.2 already placed them |
| Q12–Q14 | n | `plan.md` Slice 6, `design.md` §2 | Proof of S-05.4, the commit variable, the tag's version source | Reader's defaults adopted |
| Q18 | n (with K3) | `plan.md` 7.2 | Which links carry the key | 7.2 now names the stored log entries |
| Q22 | n | `plan.md` 7.5 | Where the operator reports the run | Reader's default fits the pack |

### Contradictions

| Item | Resolution |
|---|---|
| K1: `local_3_7` and `s_03_7` | `plan.md` 3.7 now says they are different items |
| K2: the base commit | Resolved with Q04 |
| K3: the key on all 12 links | The author confirmed all 12 stored log entries hold this key. Only links 8 and 10 also carry a fingerprint. 7.2 now says so |
| K4: 21,661 bytes | No change. The largest record is `out/two-machines-record/record.json`, not the canary |
| K5: S-05.2.e1 as structural | No change. The same `design.md` row gives the real run to Slice 7 |
| K6: presentations with no notary key | `plan.md` 5.4 now says both still judge them and must agree |
| K7: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |
| K8: 171 and 168 | No change. The 168 are among the 171 `.presentation` files |
| K9: the line limit's scope | No change. Counting all three crates is stricter than the PRD |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, identity, limits, stand-in log and report lines | yes |
| Ledger constants, link range, agent and refusal comment | yes |
| `attest` server name, signer and test paths | yes |
| CI, release, build script, release gate, CODEOWNERS, `CLAUDE.md` and canary files | yes |
| `attest/src/lib.rs:70`, `attest/build.rs:5`, `operator.pub`, the 171 count | partly. The first is now `:78` |
| The 21,661-byte record | reported no, but it holds (K4) |

## Round 14 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 3, 5 and 7 and checked 34 citations: 30 held and 4 held partly. It flagged the pack's allowed fences as code, and there is no implementation code. The author then read TLSNotary at the pinned commit `40a0695` (tag `v0.1.0-beta.1`): `crates/attestation/src/presentation.rs:82` passes the attested connection time to the certificate check, and `crates/core/src/webpki.rs:126` checks the certificate at that time.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1, F2 | n | `context.md` §4, `design.md`, `plan.md` | The verify commands and mermaid fences | Re-classed, as in earlier rounds. The pack format allows both |
| Q1 | y | `plan.md` 5.4 | Whether the certificate check uses the wall clock, which would make the 40-presentation test expire | It uses the attested connection time, so the result does not change with the date. 5.4 now says so and cites the pinned source |
| Q2 | n, re-classed by the author | `plan.md` 7.2 | Whether the log's list under the operator key equals the 12 links | Cannot be settled offline. `plan.md` Slice 7, Stop conditions, already stops if no committed real record passes against the public log. 7.2 now names this case |
| Q3–Q5, Q7–Q9, Q12–Q14 | n | `plan.md`, `design.md` | Golden names, error mapping, trait shape, return shape, roots, line count, guide format, TLS roots, operator step | Reader's defaults fit the pack |
| Q6 | n | `plan.md` 5.3 | High-S signatures | Reader's default fits the pack. The differential test and Slice 5's stop catch a disagreement |
| Q10 | n | `plan.md` 5.5 | Removing `--verify` from the monitor's audit | No change. 5.5 states it, and it is later than Slice 1's contract |
| Q11 | n | `plan.md` 7.4 | Which CI job sets `HYALOS_REQUIRE_PUBLIC_LOG` | Reader's default fits `design.md` §6 |

### Contradictions

| Item | Resolution |
|---|---|
| K1: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |
| K2: the line limit's scope | No change. Counting all three crates is stricter than the PRD |
| K3: S-03.5 before Slice 5 | No change. The Traceability table assigns S-03.5 to Slice 5 |
| K4: ADR-004 and the child process in Slices 3 and 4 | No change. `plan.md` 3.5 strips the variables from the child until Slice 5 removes it |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, profile settings, identity, limits and report lines | yes |
| Ledger constants, `openssl` sites, link range and key reading | yes |
| `attest` server name, trust read, imports, `verify` and features | yes |
| Layout, CI, release, `CLAUDE.md`, ADRs and evidence counts | yes |
| `main.rs:1087`, `ledger.rs:370`, `live_tests.rs:21`, `CODEOWNERS:10` | partly |

## Round 15 — 2026-11-23

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` at `c6ae5df` with the working copy of the pack. It thought through Slices 5 and 7 and checked 49 citations: 44 held, 4 held partly, and 1 group in the TLSNotary source could not be read by the reader. It flagged the pack's allowed fences as code, and there is no implementation code. No item is classed blocking after the re-classes below. The author read the unreadable group at the pinned commit `40a0695`: `crates/attestation/src/presentation.rs:82` holds.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1–F6 | n | `design.md`, `plan.md`, `context.md` §4 | The mermaid fences and verify commands | Re-classed, as in earlier rounds. The pack format allows both |
| Q7a | n, re-classed by the author | `plan.md` Slice 7 | Who runs the public-log check, and what happens if it fails | The pack answers it: Slice 7's Verify runs it with `HYALOS_REQUIRE_PUBLIC_LOG=1`, and 7.2 and Stop conditions stop the slice if it does not pass. The reader's default is the same |
| Q5a | n | `plan.md` 5.1 | Which crate supplies the Mozilla roots | 5.1 now names the `mozilla-certs` feature of `tlsn-core`, confirmed at the pinned commit |
| Q5b | n | `plan.md` 5.2 | No item adds the 10,000-entry limit | Reader's default adopted in 5.2 |
| Q5c, Q5d, Q5f–Q5i | n | `plan.md` Slice 5 | Non-canonical signatures, refused fields, line count, reply cap, child environment, scratch directory | Reader's defaults fit the pack |
| Q5e | n | `plan.md` 5.4 | The TLSNotary citation | Checked by the author at the pinned commit. It holds |
| Q7b–Q7f | n | `plan.md` Slice 7 | Operator evidence, literals, conditional marks, notary key, dry-run evidence | Reader's defaults fit the pack |
| Q-X1 | n (with C3) | `design.md` §6 | A later version 4 real run | Resolved with C3 |

### Contradictions

| Item | Resolution |
|---|---|
| C1: the arrow from `attest-check` to `attest` | `design.md` §1 arrow removed. `attest` keeps its own check |
| C2: the line limit's scope | No change. The count in 5.6 stands, and the reader's default also reports `hyalos-profile` |
| C3: a version 4 real run | `design.md` §6 now says it is outside this pack |
| C4: which component calls the log | `design.md` §1 arrow now starts at `hyalos-audit` |
| C5: ADR-005's text and ADR-008 | No change. ADR-005 is accepted, and its Status line names ADR-008 |

### Citations checked

| Citation | Holds |
|---|---|
| Monitor audit, profile settings, identity, limits and report lines | yes |
| Ledger constants, helpers, agent, link range and key reading | yes |
| `attest` server name, trust read, `verify`, signer, features and lockfiles | yes |
| CI, release, release gate, deny, layout, `CLAUDE.md`, CODEOWNERS and evidence counts | yes |
| `main.rs:419`, `ledger.rs:284/297/319`, `ci.yml:143`, `release.yml:207` | partly |
| TLSNotary's upstream lines | not readable by the reader. `presentation.rs:82` checked by the author |

## Result

Pass after 3 rounds.
