# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 7 |
| **Result** | pass |

## Round 1 — 2026-11-22

The reader thought through Slice 1. It checked 22 citations: 20 held and 2 held partly. It flagged two kinds of fence as code, and both are allowed by the pack format.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1 | n (re-classed) | `context.md` §4 | A `bash` fence outside a Verify heading | Stands. §4 is the pack's section for verification commands, and the fence holds only commands |
| F2 | n (re-classed) | `design.md` §1, §3, §4, §5 | `mermaid` fences counted as code | Stands. `mermaid` is an allowed tag for diagrams, and these hold diagrams |
| Q1 | y | `design.md` §2 | What a check does with a damaged state file | Fixed: refused with exit 2, file unchanged, because it could hide a latch. Reset is the way out. Added S-01.4.e3 and e4 |
| Q2 | n | `plan.md` 1.1, `design.md` R1 | The forged request's small body | Adopted: uses the probe's `SMALLEST` and fixed binding, compared with the probe's literals |
| Q3 | y | `design.md` §2, `prd.md` | Exactly 3 calls against stopping early | Fixed: at most 3, and 1 when the first control fails |
| Q4 | y | `design.md` §2, P6 | The credential's shape under test settings | Fixed: `test-` under test settings, otherwise `sk-ant-` and not `sk-ant-api` |
| Q5 | n | `design.md` §2 | Credential parsing and a missing file | Adopted: `read_private`, limit 1024, missing file refused |
| Q6 | n | `design.md` §2 | Exit code for a refusal to start | The design's 2 stands |
| Q7 | n | `design.md` §2 | The lock mechanism | Adopted: exclusive non-blocking `flock`, second check exits 2 |
| Q8 | n | `prd.md` S-01.1 | Which time a pass records | Adopted: the verdict's time, in the PRD and design |
| Q9 | n | `plan.md` 1.1, 1.2 | Test names for local criteria | Adopted: `canary_1_1_*`, `canary_1_2_*` |
| Q10 | n | `prd.md` S-01.1.e2 | How a test makes the write fail | Adopted: `<d>/canary.new` created as a directory |
| Q11 | n | `plan.md` Slice 1 | The stand-in's control and status parsing | Adopted: `--statuses`, 500 after the list, and the check parses the status itself |
| Q12 | n | `plan.md` Files | Whether `Cargo.toml` changes | Adopted: only if the build requires it |

### Contradictions

| Item | Resolution |
|---|---|
| K1: exactly 3 calls against an early stop | Design and PRD now say at most 3 |
| K2: three rules for a test credential | Design now states one rule, the probe's |
| K3: refusal exit 2 against the probe's 1 | The design stands, and the probe is unchanged |
| K4: the probe's comments name `main.rs` for items in `lib.rs` | Not a pack defect, so no edit |
| K5: the check's time against the verdict's time | PRD now says the verdict's time |

### Citations checked

| Citation | Holds |
|---|---|
| `attest/probes/hidden_header.py` three-call sequence | yes |
| `attest/src/lib.rs` `CREDENTIAL_LENGTH`, `TICKET`, `read_private`, `body`, `request`, `Trust::read`, probe-pinning test | yes |
| `attest/src/lib.rs` `pinned_credential` | partly: it has no `test-` rule, now stated in the design |
| `attest/src/bin/notary.rs` `save`, `exact`, `parse_sequences`, `spend`, `ConnectionInfo.time`, forging hook | yes |
| `kernel/src/main.rs` `HIDDEN_BYTES_LIMIT`, `CURRENT_VERSION`, `read_transcript` | yes |
| `kernel/src/live.rs` `refusal` | yes |
| `attest/src/bin/standin.rs` status control | partly: the citation now points at `serve` |
| `attest/tests/common/mod.rs` `standin`, `authorised` | yes |
| `attest/probes/two_machines.sh` `authorize` | yes |
| `attest/src/main.rs` `verify` | yes |
| The notary pack's ADR-011, and the plan's Verify artifacts | yes |

## Round 2 — 2026-11-22

The reader thought through Slice 2. It checked 18 citations: 15 held and 3 held partly. It flagged the same allowed fences as code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `design.md` §2, `plan.md` Slice 2 | Which clock the freshness gate uses, since the signed session time exists only after the session | Fixed: judged at open on the notary's clock and again at signing against the signed time. Added S-02.1.e8 and a stop condition |
| Q2 | n | `plan.md` Slice 2 | Which pass is signed if the state changes | Adopted: the pass read at open |
| Q3 | n | `plan.md` 2.1, 2.2 | Test names for local criteria | Adopted: `canary_2_1_*`, `canary_2_2_*` |
| Q4 | n | `plan.md` Slice 2 | Where the state reader lives | Adopted: `attest/src/lib.rs`, with no credential code |
| Q5 | n | `design.md` §2 | Where the window constant lives | Adopted: `CANARY_WINDOW_SECONDS` in `attest/src/lib.rs` |
| Q6 | n | `design.md` §2 | Whether the notary takes the lock | Adopted: no, the writer's rename is atomic |
| Q7 | n | `design.md` §2 | Tolerance for a pass ahead of the clock | Adopted: none |
| Q8 | n | `plan.md` Slice 2 Verify | The `grep` covers only `notary.rs` | Stands. The notary calls credential code only by naming it there |
| Q9 | n | `plan.md` 2.2 | The target version | Adopted: 0.2.0 |
| F1 to F5 | n (re-classed) | `context.md` §4, `design.md` | `bash` under the verification section, and `mermaid` diagrams | Stand, as in round 1. Both are allowed by the pack format |

### Contradictions

| Item | Resolution |
|---|---|
| C1: freshness defined on the signed time, gate run before it exists | Design now defines both checks |
| C2: gate and audit could disagree at the boundary | The notary's second check makes its signing agree with the audit |

### Citations checked

| Citation | Holds |
|---|---|
| `notary.rs` `spend`, `save`, `exact`, `parse_sequences`, `sign`, `ConnectionInfo.time`, release check | yes |
| `attest/src/lib.rs` `open`, `pinned_credential`, `CREDENTIAL_LENGTH`, `read_private`, `RELEASE`, probe-pinning test | yes |
| `attest/src/main.rs` `Word` | yes |
| `kernel/src/main.rs` `CURRENT_VERSION`, `HIDDEN_BYTES_LIMIT`; `kernel/src/live.rs` `refusal`; `kernel/src/ledger.rs` version checks | yes |
| `forged_ticket`, `two_machines.sh` `authorize` | yes |
| `standin.rs` `serve` | partly: the sequence control is Slice 1's work |
| `credential` in `attest/src/main.rs` | partly: its parsing is reused, and the file is read with `read_private` |
| The probe's comment on `CREDENTIAL_LENGTH` | partly: comment drift, not a pack defect |

## Round 3 — 2026-11-22

The reader thought through Slices 3 and 4. It checked 32 citations: 28 held and 4 held partly. It flagged the same allowed fences as code. Every slice has now been read by one reader.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 to Q5 | n (re-classed) | `design.md`, `context.md` §4 | `mermaid` diagrams, and `bash` under the verification section | Stand, as in rounds 1 and 2. Both are allowed by the pack format |
| Q6 | y | `plan.md` Slice 4 | No way to show the script proceeds without a public action | Fixed: a temporary working directory with stub executables, and the stub monitor records the call |
| Q7 | y | `design.md` §2 | `canary status` was defined only in the plan | Fixed: added to the contracts |
| Q8 | n | `plan.md` Slice 4 | Where the gate goes | Adopted: before the notary starts |
| Q9 | n | `plan.md` Slice 4 | The cron line R3 promises | Adopted: in the `RECORD.md` step |
| Q10 | y (re-classed) | `plan.md` Slice 3 | Raising the version would silently drop the operator-key check | Fixed: criterion 3.3, a stop condition and the path |
| Q11 | n | `plan.md` 3.1 | How to pin the constant across crates | Adopted: `include_str!` in a kernel test |
| Q12 | n | `plan.md` Slice 3 | The forging hook's setting | Adopted: `HYALOS_FORGE_CANARY` |
| Q13 | n | `plan.md` 3.2 | Which proof rules | Adopted: those in `read_transcript` |
| Q14 | n | `plan.md` Slice 3 | Where monitor rigs get a pass | Adopted: they write a `passed` state file |
| Q15 | n | `plan.md` Slice 3 Verify | The audit needs the public log | Stands, as the plan already says |
| Q16 | n | `plan.md` Slice 3 | UTC formatting | Adopted, with three test dates |
| Q17 | n | `plan.md` S-03.3.e3 | Which version 1 record | Default kept: `out/live3.json`, confirmed by the executor |

### Contradictions

| Item | Resolution |
|---|---|
| K1: "no pass exists" after a failed write, though an old pass stays on disk (blocking) | PRD now says the file stays as it was. The residual is design risk R7 |
| K2: a stop condition forbade the spend S-02.1.e8 requires (blocking) | The stop condition now covers refusal at open and names the exception |
| K3: `canary status` had no design row | Added, with Q7 |
| K4: where the state reader starts | Slice 1 now puts it in `attest/src/lib.rs` |
| K5: the design implied reusing a parser that takes no path | Design now says this is new code using the same line rules |
| K6: the operator check carried over only in `context.md` | Fixed, with Q10 |
| K7: "proceeds" against no public action (blocking) | Fixed, with Q6 |
| K8: "no network" was too broad | Design narrowed to deciding freshness |

### Citations checked

| Citation | Holds |
|---|---|
| `kernel/src/main.rs` `CURRENT_VERSION`, `read_transcript`, `HIDDEN_BYTES_LIMIT`, `audit_report` | yes |
| `kernel/src/main.rs` `witnessed` | partly: run and position checks are in `check_answer` |
| `kernel/src/ledger.rs` version gate and operator check | yes |
| `attest/src/lib.rs` `read_private`, `CREDENTIAL_LENGTH`, probe test, `TICKET`, `read_ticket`, `open` | yes |
| `attest/src/lib.rs` `pinned_credential` | partly: it has no `sk-ant-` prefix check, now stated |
| `attest/src/bin/notary.rs` `save`, `spend`, `ConnectionInfo.time`, forging hook; `forging-notary.rs` | yes |
| `attest/src/main.rs` `credential` | partly: takes no path, now stated |
| `attest/src/main.rs` `Word`, `kernel/src/live.rs` `refusal`, `standin.rs` `serve`, `common/mod.rs` `authorised` | yes |
| `attest/probes/two_machines.sh` `authorize` | partly: the gate's place is now stated |
| Committed records, `RECORD.md`, the notary pack's ADRs, `attest/Cargo.toml`, `kernel/Cargo.toml`, the probe's prefix rule | yes |

## Round 4 — 2026-11-22

The reader thought through Slices 2, 3 and 4. It checked 38 citations: 37 held and 1 held partly. It flagged the same allowed fences as code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `design.md` R7, `prd.md` S-01.2, ADR-003, ADR-004 | A failed latch write could leave a fresh pass for the notary to serve | Fixed without changing a decision: non-pass results are staged before the first call, so recording one is a rename, with removal of the state as the fallback. Added S-01.2.e6, S-01.2.e7 and a stop condition |
| Q2 to Q6 | n (re-classed) | `context.md` §4, `design.md` | `bash` under the verification section, and `mermaid` diagrams | Stand, as in earlier rounds. `pack-format.md` §6 allows both |
| Q7 | n | `plan.md` Slice 2 | How the pass reaches `sign` | Adopted: carried in the session |
| Q8 | n | `plan.md` Slice 2 | The refusal of a requester extension | Adopted: names the existing test |
| Q9 | n | `plan.md` Slice 3 | The wrong-length hook value | Adopted: `malformed` is 7 bytes |
| Q10 | n | `plan.md` Slice 3 | How a record gets two pass times | Adopted: the rig rewrites the state between sessions |
| Q11 | n | `design.md` §2 | Where the report line goes with no attempt line | Stands: every passing version 3 record has an attempt line |
| Q12 | n | `plan.md` 3.2 | The proof gate's version list | Adopted: named in 3.2 |
| Q13 | n | `plan.md` Slice 4 | How the stubbed script tests differ | Adopted: stub `ssh` exits as the test sets, and `canary status` has its own tests |
| Q14 | n | `plan.md` Slice 4 | The script's check for built programs | Adopted |
| Q15 | n | `plan.md` Slice 4 | The cron line's credential path | Adopted: placeholders |

### Contradictions

| Item | Resolution |
|---|---|
| K1: R7 let a pass survive a non-pass verdict (blocking) | Fixed, with Q1 |
| K2: S-01.1.e2 against R7 | Resolved: S-01.1.e2 covers only a pass that cannot be written, and R7 now covers non-pass results |
| K3: P6 cited the wrong risk | Now cites the `canary check` contract |
| K4: the probe's comments name the wrong file | Not a pack defect, so no edit |

### Citations checked

| Citation | Holds |
|---|---|
| `attest/src/bin/notary.rs` `spend`, `sign`, `save`, `exact`, `parse_sequences`, `ConnectionInfo.time`, forging hook | yes |
| `attest/src/bin/notary.rs` `main` dispatch | partly: its body was not read |
| `attest/src/lib.rs` and `attest/src/main.rs` items the pack names | yes |
| `standin.rs`, `common/mod.rs`, `two_machines.sh`, `hidden_header.py` | yes |
| `kernel/src/main.rs`, `ledger.rs`, `live.rs`, `forgery_tests.rs` items the pack names | yes |
| Committed records and keys, `RECORD.md`, `attest/Cargo.toml`, and the earlier packs' ADRs | yes |

## Round 5 — 2026-11-22

The reader thought through Slices 1 and 4. It checked 28 citations: 26 held and 2 held partly. It flagged the same allowed fences as code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` Slice 1 | No named way to fail one rename in a test | Fixed: a setting honoured only under the default-off `canary-test-hooks` feature, a Verify line, and criterion 1.3 |
| Q2 | y | `design.md` R7, ADR-004 | A failed latch rename could lose the latch, so a later pass would clear it | Fixed without changing a decision: a `canary.init` marker makes a lost state refuse every check until the customer's reset. Added S-01.4.e5 |
| Q3 | n | `design.md` R1, `plan.md` | The probe's literals | Adopted: copied verbatim |
| Q4 | n | `design.md` integration | The TLS client | Adopted: `tokio-rustls` with `Trust::read`'s roots |
| Q5 | n | `design.md` §2 | Staging file names | Adopted: all named in §2 |
| Q6 | n | `design.md` §2 | Exit code when rename and removal both fail | Adopted: 2 |
| Q7 | n | `plan.md` Slice 4 | What the stubs must print | Adopted, with a named skip when `/usr/bin/time` is absent |
| Q8 | n | `design.md` security | Who can reach the notary machine | Adopted: the design and `RECORD.md` step state that the script's gate proves no independence |
| Q9, Q10 | n (re-classed) | `design.md`, `context.md` §4 | `mermaid` diagrams, and `bash` under the verification section | Stand, as in earlier rounds. `pack-format.md` §6 allows both |

### Contradictions

| Item | Resolution |
|---|---|
| K1: R7 accepted losing a latch, against ADR-004 (blocking) | Fixed, with Q2 |
| K2: the operator "has no write access", though the script reaches the notary machine over ssh | The security section now states the claim's condition |
| K3: the citation for the version gate | Stands: the context cites the function, and the gate is two lines into it |
| K4: staging names differ between plan and design | Fixed, with Q5 |

### Citations checked

| Citation | Holds |
|---|---|
| `attest/src/bin/notary.rs` `spend`, `save`, `ConnectionInfo.time`, forging hook | yes |
| `attest/src/lib.rs` and `attest/src/main.rs` items the pack names | yes |
| `kernel/src/main.rs` `CURRENT_VERSION`, `HIDDEN_BYTES_LIMIT`, `ledger.rs`, `live.rs` | yes |
| `kernel/src/main.rs` `read_transcript` version gate | partly: two lines into the cited function |
| `attest/src/bin/standin.rs` `serve` | partly: `--statuses` is Slice 1's work |
| `hidden_header.py`, `common/mod.rs`, `two_machines.sh`, `authorise.rs`, `RECORD.md`, committed records, earlier packs' ADRs | yes |

## Round 6 — 2026-11-22

The reader thought through Slice 1. It checked 19 citations: 15 held, 3 held partly, and 1 did not hold, a stale comment in the existing probe. It flagged the same allowed fences as code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y, developer-only | `design.md` §2, R7, `plan.md` Slice 1 | If the directory refuses both rename and removal, an earlier pass survives, against ADR-003 | The developer accepted it as a loud, stated limit on 2026-11-22. Written as ADR-007, which amends ADR-003, with the stop condition and `RECORD.md` step updated |
| Q2 | y (re-classed) | `design.md` §2, §4 | What a non-pass check writes over a latch | Fixed: every check keeps a latch, and exits 1 |
| Q3 | n | `design.md` §2 | Refusal order | Adopted: lock, state read, credential |
| Q4 | n | `design.md` §2 | Whether the state reader uses the `read_private` rules | Adopted: yes |
| Q5 | n | `plan.md` Slice 1 | The test setting's meaning | Adopted |
| Q6 | n | `plan.md` Slice 1 | `--statuses` beside `--status` | Adopted: it overrides |
| Q7 | n | `plan.md` 1.2 | How the test holds the lock | Adopted: the test takes the `flock` itself |
| Q8 | n | `plan.md` Slice 1 | Which head writer the forged request uses | Adopted: copies the probe's `head` |
| Q9 | n | `design.md` §2 | A missing credential file | Already stated: refused |
| Q10 | n | `design.md` §2 | Reset on a `passed` state | Adopted: it writes `held` |
| Q11 | y (re-classed) | `design.md` §2, ADR-003 | A crash on a first check left a marker that blocked the next one | Fixed: the marker is staged and made real only when a verdict is recorded |
| F1 to F5 | n (re-classed) | `context.md` §4, `design.md` | `bash` under the verification section, and `mermaid` diagrams | Stand, as in earlier rounds. `pack-format.md` §6 allows both |

### Contradictions

| Item | Resolution |
|---|---|
| C1: a non-pass result could be read as overwriting a latch | Fixed, with Q2 |
| C2: the stop condition against the design's double-failure path (blocking) | Fixed, with Q1 |
| C3: a crash changed state through the marker (blocking) | Fixed, with Q11 |
| C4: the probe's comments name the wrong file | Not a pack defect, so no edit |

### Citations checked

| Citation | Holds |
|---|---|
| `attest/src/lib.rs` `CREDENTIAL_LENGTH`, `read_private`, `Trust::read` | yes |
| `attest/src/lib.rs` `pinned_credential` | partly: the prefix rules are new code, as the design says |
| `attest/src/lib.rs` probe-pinning test | partly: correct in the pack, and the probe's own comment names the wrong file |
| `attest/src/bin/notary.rs` `save`, `exact`, `parse_sequences`, forging hook | yes |
| `attest/src/main.rs` `credential`, `hidden_header.py` literals and verdicts, `common/mod.rs` | yes |
| `attest/src/bin/standin.rs` | partly: `--statuses` is Slice 1's work |
| `kernel/src/main.rs`, `kernel/src/ledger.rs`, `attest/Cargo.toml`, the Verify artefacts | yes |
| The probe's comment on `CREDENTIAL_LENGTH` | no: comment drift outside the pack |

## Round 7 — 2026-11-22

The reader thought through Slice 1 and said it has no blocking questions of its own. It checked 23 citations: 19 held, 3 held partly, and 1 did not hold, the stale comment in the existing probe. It flagged the same allowed fences as code.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 to Q5 | n (re-classed) | `context.md` §4, `design.md` | `bash` under the verification section, and `mermaid` diagrams | Stand. `pack-format.md` §6 allows `bash` and `mermaid`, and these hold only commands and diagrams |
| Q6 | n | `prd.md` S-01.3 | "Checked first" against the design's order | Adopted: the PRD now says before any provider call |
| Q7 | n | `design.md` §2 | A failed pass write left a marker with no state | Adopted: a pass writes the marker after the state |
| Q8 | n | `plan.md` Slice 1 | The status list across connections | Adopted: its position is shared |
| Q9 | n | `plan.md` Slice 1 | The reason given for copying the head was wrong | Adopted: reason corrected |
| Q10 | n | `design.md` §2 | `Sequences::save` is private | Adopted: the pattern is re-implemented in `attest/src/lib.rs` |
| Q11 | n | `design.md` §2 | The credential parser | Already stated as new code |
| Q12 | n | `prd.md` | Wording of refusals | The reader's default stands: the probe's `refused:` form |
| Q13 | n | `design.md` §2 | What reset does with staged files | The reader's default stands: leaves the marker, and staged files are removed on exit |
| Q14 | n | `prd.md` S-01.2.e5 | `--drop` with a status list | The reader's default stands: a separate test using `--drop` |
| Q15 | n | `design.md` integration | Address and read bound | The reader's default stands: `Trust::read`, and a 64-byte status line |

### Contradictions

| Item | Resolution |
|---|---|
| K1: "credential first" against the design's order | PRD and plan reworded, non-blocking |
| K2: the plan's false reason | Corrected, non-blocking |
| K3: a failed pass write and the marker | Fixed, with Q7, non-blocking |
| K4: the probe's comments name the wrong file | Not a pack defect, so no edit |
| K5: reset shown only from `latched` | Diagram now shows it from every state |

### Citations checked

| Citation | Holds |
|---|---|
| `attest/src/lib.rs` `read_private`, `CREDENTIAL_LENGTH`, `body`, `Trust::read`, probe test, `answer`, `TICKET`, `read_ticket` | yes |
| `attest/src/lib.rs` `pinned_credential`, `request` | partly: as the design describes |
| `hidden_header.py` literals and `head` | yes |
| `hidden_header.py` comments | no: comment drift outside the pack |
| `attest/src/main.rs` `credential`, `notary.rs` `save`, `exact`, `parse_sequences`, `common/mod.rs`, `Cargo.toml` | yes |
| `standin.rs` `serve` | partly: `--statuses` is Slice 1's work |
| `kernel/src/main.rs`, `ledger.rs`, `authorise.rs`, `two_machines.sh`, the Verify inputs | yes |

## Result

Pass after 7 rounds.
