# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 4 |
| **Result** | Pass after 4 rounds |

## Round 1 — 2026-11-22

Reader: Haiku (Agent tool, model: haiku, Explore). It thought through Slice 1 and Slice 2 and checked 40 citations: 34 held, 6 partly, none failed. It reported every `mermaid` fence and the `bash` fences in `context.md` as code. Neither is: the pack format allows both tags and each holds a diagram or shell commands.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1–Q7 | n (re-classed) | `design.md`, `plan.md` | Mermaid fences reported as code | Re-classed: the pack format, §6, allows `mermaid` for diagrams, and these hold only diagrams. No edit |
| Q8–Q11 | n (re-classed) | `context.md` §4 | `bash` fences outside a Verify heading | Re-classed: §4 is the pack's statement of its verify commands, as the template requires, and these hold only commands. No edit |
| Q12 | y | `plan.md` Slice 2, `design.md` §2, ADR-007 | Whether in-process replay reproduces records made on KVM | Tested on 2026-11-22 at `72136c1`: all 13 committed transcripts that parse replayed identically on both backends. `load_backend` uses KVM only when `HYALOS_KERNEL` is set (`spikes/task-kernel/kernel/src/main.rs:753`). `design.md` now states that the audit always uses `Backend::Process`, with this evidence |
| Q13 | n | `design.md` §2 | Which bytes `operator` hashes | Default adopted: the key's DER bytes, in lowercase hex |
| Q14 | n | `design.md` §2 | `--operator-public` is optional today | Default adopted: a version 2 audit without it is refused |
| Q15 | n | `design.md` §2 | How the guest hash enters the task identity | Default adopted: 32 raw bytes in, 64 lowercase hex out |
| Q16 | n | `design.md` §2 | Whether the intent's `task` is the version 2 identity | Default adopted: yes |
| Q17 | y (re-classed) | `plan.md` Slice 1, S-01.7.e2 | Until Slice 5, a version 2 pass would print the line reserved for a budgeted pass | Re-classed: the default would ship a misleading verdict. Slice 1 now prints `audit passed: attempt budget not yet checked`, and Slice 5 proves S-01.7.e2 when it changes that to `audit passed` |
| Q18 | y (re-classed) | `plan.md` Slices 4 and 5, S-01.6 | A legacy audit could pass ticketed attestations between Slices 4 and 5 | Re-classed for the same reason. S-01.6 moved to Slice 4, which now depends on Slice 1 |
| Q19 | n | `design.md` §2 | The identifier's size and source | Default adopted: 32 bytes from the operating system's random source |
| Q20 | n | `plan.md` Shared contract | Local criteria's test names | The Shared contract now defines `record_N_k_<what>` |
| Q21 | n | `design.md` §2, S-06.3 | The design lists more malformed cases than the PRD | S-06.3 now points at the design's full list |
| Q22 | n | `design.md` §2 | The shape of `verify`'s `ticket` object | Default adopted: `authorisation`, `task`, `budget`, `round`, `prior`, `ticket` |

### Contradictions

| Item | Resolution |
|---|---|
| C1: ADR-011 reserves `audit passed`, but Slice 1 prints it with no budget | Blocking. Resolved with Q17 in `plan.md` |
| C2: test names for local criteria break the Shared contract | Resolved with Q20 |
| C3: pre-mortem P1 names a slice without its stop | `design.md` P1 now names both slices, and Slice 4 carries the stop |
| C4: the audit's backend and the Verify's backend treated as interchangeable | Resolved with Q12. The Verify runs in-process unless `HYALOS_KERNEL` is set |
| C5: the README's Route names the slices differently from the plan | README Route updated |
| C6: the README says no open questions while Q12 stood | Q12 resolved, so the README stands |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/kernel/src/main.rs:766` | partly: needs the version branch the pack adds |
| `spikes/task-kernel/kernel/src/main.rs:771` | partly: the monitor's entry point |
| `spikes/task-kernel/kernel/src/ledger.rs:371` | partly: one example of the pattern |
| `spikes/task-kernel/kernel/src/audit_tests.rs:23` | partly: corrected to line 12 |
| `spikes/task-kernel/attest/src/bin/notary.rs:530` | partly: corrected to line 640 |
| `spikes/task-kernel/attest/src/main.rs:316` | partly: corrected to line 318 |
| The other 34 citations across `main.rs`, `ledger.rs`, `live.rs`, `notary.rs`, `lib.rs`, the manifests, `CLAUDE.md` and the notary pack | yes |

## Round 2 — 2026-11-22

Reader: Haiku (Agent tool, model: haiku, Explore). It thought through Slice 3 and Slice 4 and checked 27 citations: 21 held, 6 partly, none failed. It read some citations from the plan-record worktree rather than its own tree, both at `72136c1`. It reported the `mermaid` fences and the `bash` fences in `context.md` as code, which round 1 re-classed for the same reason.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q4.1 | y | `prd.md` S-04.4.e2, `design.md` §2 | A malformed identifier answers `authorisation` or drops the link | The PRD stands. With a valid run, a missing or malformed identifier answers `authorisation`. A line over the limit or with a bad run drops the link |
| Q4.2 | y | `design.md` R1 and §6, `plan.md` Slice 4, `prd.md` Assumptions | A real session is required, and no test may contact the real provider | "Real" now means a real MPC-TLS session between the built programs against the stand-in provider |
| Q3.1 | n | `design.md` §2 | Authorisation file grammar | The pack stands: its lines are listed in order and `end` is bare |
| Q3.2 | y (re-classed) | `design.md`, Deploy order | Authorising straight after an upgrade is refused until the notary has started once | Deploy order now says to start it once first |
| Q3.3 | n | `design.md` §2 | Who creates the directory | Default adopted: `authorize`, mode 700 |
| Q3.4 | n | `design.md` §2 | Random source | Default adopted: `getrandom`, refusing on a short read |
| Q3.5 | n | `design.md` §2 | `notary tickets` on a version 1 or missing file | Default stands: refused with no output, under the existing no-partial-total rule |
| Q3.6 | n | S-03.5.e2 | The half-written state cannot arise from `keep_private` | Default adopted: the test plants the file |
| Q3.7 | n | `plan.md` Slice 3 | How to force the race | Default stands, as the dry run says |
| Q4.3 | n | `design.md` §4 | Precedence of refusals | Default adopted and stated in Spend |
| Q4.4 | n | `design.md` §2 | Hex fields' encoding | Default adopted: ASCII, which gives 144 bytes |
| Q4.5 | n | `design.md` §2 | Overflow of round and prior total | Default adopted: `authorize` refuses |
| Q4.6 | n | `design.md` §2 | How `ask` gets the identifier | The pack stands: `HYALOS_NOTARY_AUTHORISATION` joins `NOTARY_SETTINGS` |
| Q4.7 | n | `plan.md` Slice 4 | How spend's outputs reach `sign` | Left to the executor, since it changes no contract |
| Q4.8 | y (re-classed) | `design.md` §2 | The existing revert restores only the count, which would leave a ticket on a failed save | Spend now restores the run's whole entry |
| Q4.9 | n | `design.md`, Deploy order | A new tool against an old notary is dropped, not told `release` | Deploy order now states both directions. Both fail closed |
| Q4.10 | n | `plan.md` Slice 4 | Version to bump to | Default adopted: `0.1.0` |
| Q4.11 | n | `design.md` §2 | Outcome strings | Default stands: the reason names |
| Q4.12 | n | `context.md` §3 | Whether TLSNotary appends and refuses requester extensions | Read at the pinned commit: `builder.rs:110` appends and `config.rs:91` refuses by default. Slice 4 still proves it |
| F1–F11 | n (re-classed) | `context.md` §4, `design.md`, `plan.md` | Fences reported as code | As round 1: the format allows these tags, and they hold only commands or diagrams. No edit |

### Contradictions

| Item | Resolution |
|---|---|
| C1: malformed identifier | Blocking. `design.md` changed (Q4.1) |
| C2: real session against no real provider | Blocking. `design.md`, `prd.md` and `plan.md` changed (Q4.2) |
| C3: deploy order against the migration | `design.md` changed (Q3.2) |
| C4: the release check in one direction | `design.md` changed (Q4.9) |
| C5: a half-written file cannot arise | `design.md` says the test plants it (Q3.6) |
| C6: context states as fact what the PRD assumes | `context.md` changed (Q4.12) |
| C7: README says no open questions | Q4.1 and Q4.2 resolved, so the README stands |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/lib.rs:457` | partly: new words need `LinkFailure` variants |
| `spikes/task-kernel/kernel/src/live.rs:27` | partly: the array grows to five |
| `spikes/task-kernel/kernel/src/main.rs:507` | partly: use by both reports not checked by the reader |
| `spikes/task-kernel/out/live3.json`, `guest/digest.wasm` | partly: not opened by this reader, exercised by the author in round 1 |
| TLSNotary builder and default validator | partly: outside its tree, read by the author at `40a0695` |
| The other citations across `notary.rs`, `lib.rs`, `main.rs`, `live.rs`, `ledger.rs`, the test rigs, manifests and `CLAUDE.md` | yes |

## Round 3 — 2026-11-22

Reader: Haiku (Agent tool, model: haiku, Explore). It thought through Slice 5 and Slice 6 and checked 28 citations: 26 held, 1 partly, 1 did not hold because it points into the pinned TLSNotary repository. It reported the `mermaid` and `context.md` `bash` fences as code, re-classed as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1–F11 | n (re-classed) | `design.md`, `plan.md`, `context.md` | Fences reported as code | As round 1. No edit |
| S5-1 | y, developer-only | `plan.md` Slice 5, S-05.1, S-05.4 | How a test gets a validly signed bad ticket | Developer, 2026-11-22: a test-only build. `design.md` §6 defines `forging-notary` behind the off-by-default `forge-ticket` feature, and Slice 5 gains criterion 5.1 and a Verify that the release `notary` has no path to it |
| S5-2 | n | `plan.md` Slice 5 | Where S-05.7.e4 and e8 are tested | Default adopted. Slice 5's Verify already searches both test trees |
| S5-3 | y (re-classed) | `design.md` §2 | The agreement check omits `prior`, which the report states | `prior` added to Ticket agrees |
| S5-4 | n | `design.md` §2 | Order of the report's lines | Default adopted, left to the executor within the listed contents |
| S5-5 | n | `kernel/src/main.rs:579` | How agreed values cross the loop | Left to the executor, since it changes no contract |
| S5-6 | n | S-05.7.e5 | An edited attestation is refused earlier by its hash | `design.md` §6 says the test updates the hash and re-signs the chain |
| S6-1 | y (re-classed) | `plan.md` Slice 6 | Replay rebuilds answers from fixed fields, so a proof would diverge | Slice 6 step 2 and the design's invariant now say replay copies it |
| S6-2 | n | `plan.md` Slice 6 | How tests build a record with a proof | Default adopted: a `Rig` helper |
| S6-3 | n | `design.md` §2 | What `sha256` covers | Default adopted: the proof bytes as supplied |
| S6-4 | n | `design.md`, public-api | Scope of `RECORD.md` | Default adopted: adds the link and both state files |
| S6-5 | n | `design.md` §2 | Proof count per answer or total | Default adopted: total, zero included |
| S6-6 | n | `design.md` §2 | Wording of the proof line | Changed from the default: `unchecked provider proofs: {n}, which prove nothing`, so the report still has seven `not proven` limits |
| S6-7 | n | `plan.md` Slice 6 | Where version 1 refuses proofs | Default adopted: `audit_record` |
| P-1 | n | `context.md` §3 | The citation is outside the repository | Citation now names the repository, tag, commit and pin |
| P-2 | y (re-classed) | `plan.md` 1.1 | The interim first line begins with the prefix ADR-011 reserves for a budgeted pass | Round 1's fix was wrong. The line is now `interim audit passed: attempt budget not yet checked` |
| P-3 | n | S-05.10.e1, e2 | Run-count wording | Default adopted: existing wording plus the qualifier |

### Contradictions

| Item | Resolution |
|---|---|
| K1: scope of `RECORD.md` | `design.md` changed (S6-4) |
| K2: `prior` not in the agreement check | Blocking. `design.md` changed (S5-3) |
| K3: S-05.7.e5's refusal | `design.md` §6 changed (S5-6) |
| K4: replay and the proof field | Blocking. `plan.md` and `design.md` changed (S6-1) |
| K5: run-count wording | The PRD row gives the count, and the design's wording stands (P-3) |
| K6: TLSNotary citation | `context.md` changed (P-1) |
| K7: the proof count before Slice 6 | Slice 5's Contracts say it arrives in Slice 6 |

### Citations checked

| Citation | Holds |
|---|---|
| `crates/attestation/src/builder.rs:110` | no in this repository: it is in the pinned TLSNotary checkout, and the citation now says so |
| `spikes/task-kernel/kernel/src/main.rs:507` | partly: use by both reports not checked by the reader |
| The other 26 citations across `main.rs`, `ledger.rs`, `live.rs`, `notary.rs`, `lib.rs`, the test rigs, `CLAUDE.md` and the notary pack | yes |

## Round 4 — 2026-11-22

Reader: Haiku (Agent tool, model: haiku, Explore), in a second run of this skill after the developer answered S5-1. It thought through Slice 5 and Slice 6, the slices round 3's fixes touched, and checked 31 citations: 28 held, 2 partly, 1 not checked because it is in the pinned TLSNotary repository, whose lockfile entry the author confirmed at `40a0695`. It reported the `mermaid` and `context.md` `bash` fences as code, re-classed as in round 1. It found no other blocking item.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q-01–Q-11 | n (re-classed) | `design.md`, `plan.md`, `context.md` | Fences reported as code | As round 1. No edit |
| Q-12 | n | `plan.md` Slice 5 | A second binary cannot call the notary's session | Reader's default not adopted, since it moves trusted code. `forging-notary` includes `notary.rs`'s source and the hook is compiled only under the feature |
| Q-13 | n | `plan.md` Slice 5 | `witnessed` also serves `attested`, which has no task | Default adopted in the suggested path |
| Q-14 | n | `plan.md` Slice 5 | How tests reach the forging program | Default adopted: the variable is set on that child only |
| Q-15, Q-16, Q-17, Q-18, Q-19 | n | `plan.md` Slice 5, `design.md` §2 | Refusal wording, line order, where range is checked, the operator row, where S-05.7.e4 sits | Defaults adopted. No edit needed |
| Q-20 | n | `plan.md` Slice 6 | Where a proof entry is validated | Default adopted: one parse for all three commands |
| Q-21, Q-22, Q-23, Q-24 | n | `design.md` §2, `plan.md` Slice 6 | Version 1 refusal, replay mapping, the grep phrase, the local ID | Defaults adopted. No edit needed |

### Contradictions

| Item | Resolution |
|---|---|
| C-01: the design also requires `prior` to agree | Non-blocking. The design is stricter, so meeting it meets S-05.2 |
| C-02: `notary tickets` also prints the identifier | Non-blocking. The design is a superset of S-03.7 |
| C-03: Slice 1's interim first line against S-01.7.e2 | Non-blocking. Slice 1 takes only row e1 and Slice 5 proves e2 |
| C-04: ADR-001's Consequences against ADR-008 | Non-blocking. ADR-001's Status line records the amendment, and an accepted ADR is not edited |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/kernel/src/ledger.rs:371` | partly: one example of the pattern |
| `spikes/task-kernel/attest/src/bin/notary.rs:640` | partly: the dispatch point |
| `crates/attestation/src/builder.rs:110` in TLSNotary | not checked by the reader: outside its tree |
| The other 28 citations | yes |

## Result

Pass after 4 rounds.
