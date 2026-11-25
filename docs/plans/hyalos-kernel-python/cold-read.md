# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 6 |
| **Result** | Pass after 6 rounds (3 in this run) |

## Round 1 — 2026-11-25

Reader: Haiku (Agent tool, model: haiku, Explore), on a detached tree of `origin/main` with the pack
copied in. It thought through Slice 1 and Slice 2, checked 36 citations (33 held, 3 partly) and
found no code. It flagged six fences as code; they are allowed by `pack-format.md` §6 and re-classed
below. Two items changed accepted ADRs and were put to the developer before any fix.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `design.md` §2 Python surface; ADR-004; `plan.md` Slice 2 | The SDK's shape: members on `task`, or bare names; which module the script imports | `design.md` §2's row is now "Generated SDK module `hyalos`": one public name `task`, scripts begin `from hyalos import task`, with `task`'s members listed. PRD S-02.2 and S-02.2.e1 and Slice 2's 2.2 restated to match |
| Q2 | n | `design.md` §2 Draft contract | Whether empty lists are valid | Reader's default adopted: an empty list is valid and grants nothing, written into `design.md` §2 |
| Q3 | n | `design.md` §2 Draft contract | An unsupported value: `BadContract` or `UnknownSchema` | Reader's default adopted: not a contract, so `BadContract` before the guest loads, written into `design.md` §2 |
| Q4 | n, re-classed y developer-only | ADR-001; `plan.md` Slice 1 | ADR-001 gates the feature on `task-kernel` and `hyalos-audit`, but spike profiles live in `hyalos-profile`, so they would compile into default builds | Developer answered 2026-11-25: amend. ADR-006 (accepted) adds the feature to `hyalos-profile`; ADR-001's Status line gains the amendment clause; Slice 1 path, Verify and `context.md` §2 restated |
| Q5 | n | `plan.md` Slice 1 Verify | 1.3 names the `hyalos-audit` feature, Verify does not | Reader's default adopted: Slice 1's clippy and test lines add `hyalos-audit/spike-contract` and `hyalos-profile/spike-contract` |
| Q6 | n | `plan.md` Slice 1; `design.md` §2 KVM rows | No mechanism for a kernel image to name its profile; `hyalos.spike/1` already passes `same_settings` | Reader's default adopted: Slice 1 does not touch the kernel image or `on_backend`; the first slice adding a profile with other settings adds the mechanism (`design.md` §2, `context.md` §2, Slices 3 and 4) |
| Q7 | n | `design.md` §2 `Reason::BadContract` | `REASONS` is a fixed `[Reason; 29]` | Reader's default adopted: the variant, its entry and the array length are behind the feature; a default build's table is unchanged |
| Q8 | n | `design.md` §2 Spike run command | The command's name and argument order | Named `spike-run`, with the argument order, and the profile chosen by `HYALOS_SPIKE_PROFILE` (default `hyalos.spike/1`) |
| Q9 | n | `design.md` §2 Spike run command | Size limits for records 0 to 2 | No new limit beyond the contract's 4,096 bytes and the backends' existing record limits, written into the row |
| Q10 | n | `hyalos-os/ABI.md`; `plan.md` Slice 1 | Whether `BadContract` is listed in `ABI.md` | Not listed; `ABI.md` is not edited; ADR-0136 records it (Slice 1's 1.2) |
| Q11 | n | `plan.md` Slice 2 2.2 | Whether builtins count as names outside the SDK | Builtins allowed, nothing else imported, written into 2.2 |
| Q12 | n | `plan.md` Slice 2 2.3 | The fake `_hyalos` must serve placeholders at 0 to 2 | Written into 2.3, with the end condition and the trailing newline |
| Q13 | n | `plan.md` Slice 2 Verify | How public names are listed | "Public" defined in `design.md` §2 as names not starting with `_`; how `check.sh` lists them stays the executor's |
| Q14 | n | `plan.md` Slice 2 Verify | Output paths in `/tmp` | Verify now writes to `$(mktemp -d)` |
| Q15 | n | `plan.md` Slice 2 Verify | The digest guest counts records 0 to 2 | Verify now says the count is `N+3` and asserts nothing about it |
| Q16 | n, re-classed y | `plan.md` Slice 2 dry run | The S-02.4 dry run needed a WAT guest, but Slice 2 may not change `hyalos-os/` | Added `spike-task` (`design.md` §2, Slice 1 contract and local criterion 1.5); Slice 2's Verify proves S-02.4 with it |
| Q17 | n | `design.md` §2; PRD S-02.1.e2 | Normalise or refuse a non-canonical contract | Already answered: S-02.1.e2 and `design.md` §2 say refuse; now restated for the generator |
| Q18 | n | `design.md` §2 Generator | The SDK file's layout | Left to the executor: only its properties are contract; the reader's default (fixed template, contract order) is fine |
| F1 | n (re-classed) | `context.md` §4 | A `bash` fence not under a Verify heading | Allowed: `context.md` §4 is the pack's verification section, which `spec` requires to hold the exact commands; the fence holds commands only |
| F2 | n (re-classed) | `design.md` §1 | `mermaid` fence | Allowed by `pack-format.md` §6: a diagram, not code |
| F3 | n (re-classed) | `design.md` §3 | `mermaid` fence | As F2 |
| F4 | n (re-classed) | `design.md` §4 | `mermaid` fence | As F2 |
| F5 | n (re-classed) | `design.md` §5 | `mermaid` fence | As F2 |
| F6 | n (re-classed) | `plan.md` Dependency graph | `mermaid` fence | As F2 |

### Contradictions

| Item | Resolution |
|---|---|
| K1 (y, developer-only): `design.md` and Slice 3 name the native call `finish`; ADR-004 names it `return`, a Python keyword | Developer answered 2026-11-25: name it `finish`. ADR-005 (accepted) amends ADR-004, whose Status line gains the clause; `design.md` unchanged |
| K2: ADR-004's `task.…` against `design.md`'s bare names | As Q1: `design.md` changed to `task.…` |
| K3: ADR-001's crate list against Slice 1's `hyalos-profile` | As Q4: ADR-006 |
| K4: Slice 1 1.3 against its Verify | As Q5: Verify changed |
| K5: `ABI.md:70-71` (one line per record) against multi-line records 0 to 2 | `design.md` §2 gains "Record form under a spike profile"; ADR-0136 states the exception (1.2); `ABI.md` not edited |
| K6: "harmless" claim about the digest guest's count | As Q15 |
| K7: Slice 1 changing the KVM check, not needed for `hyalos.spike/1` | As Q6 |
| K8: ADR numbering in Slices 4 and 5, and which ADR-002 is meant | Slice 4 now creates a repository ADR at the next free `docs/adr/` number; Slice 5's findings ADR is the next free pack number (ADR-007 now); "this pack's ADR-002" written where meant |
| K9: upstream pins unresolved while Open questions says none | Not an open question: the pin is the executor's choice, now required to be recorded in `build.sh`, `README.md` and the progress file (Slice 3's 3.1; Slice 4's 4.1 already says so) |

### Citations checked

| Citation | Holds |
|---|---|
| `hyalos-os/audit/src/replay.rs:118-124`, `:144-153`, `:378-404`, `:236-246`, `:94` | yes |
| `hyalos-os/audit/src/record.rs:21-52`, `:205`, `:256-276`, `:150-189` | yes |
| `hyalos-os/profile/src/lib.rs:35-71`, `:97-101` | yes |
| `hyalos-os/monitor/src/main.rs:182-189`, `:155-171`, `:205-229`, `:682` | yes |
| `hyalos-os/monitor/src/runner.rs:65-95`, `:400-407` | yes |
| `hyalos-os/monitor/src/runner.rs:633` | partly: a test-switches trace printer; Slice 5's pattern line now says so |
| `hyalos-os/monitor/src/kvm.rs:29-41`, `:33`, `:332-353` | yes |
| `hyalos-os/kernel-image/src/main.rs:32-35`, `:233-246` | yes |
| `crates/xtask/src/hyalos.rs:14`, `:18` | yes |
| `hyalos-os/guest/src/lib.rs:164-208`, `:14-24` | yes |
| `hyalos-os/ABI.md:29-32` | yes |
| `hyalos-os/ABI.md:70-71` | partly: conflicts with spike records 0 to 2 (K5) |
| `hyalos-os/monitor/src/ring_tests.rs:58`, `abi_tests.rs:161-177`, `tests/release_switches.rs:82` | yes |
| `.github/workflows/ci.yml:284`, `hyalos-os/evidence/fixtures/issues.tsv`, `spikes/73/*` | yes |

## Round 2 — 2026-11-25

Reader: Haiku (Agent tool, model: haiku, Explore), a new reader on a detached tree of `origin/main`
with the pack copied in. It thought through Slice 3 and Slice 4, checked 38 citations (29 held, 7
partly, 1 not checked: Monty's tag, which needs the network) and found no code. It flagged the same
six fences as round 1, re-classed the same way. One item changed an accepted ADR and was put to the
developer before any fix.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1 | n (re-classed) | `context.md` §4 | `bash` fence not under a Verify heading | As round 1, allowed; a `### Verify` heading now sits above it so it reads as one |
| F2–F6 | n (re-classed) | `design.md` §1, §3, §4, §5; `plan.md` Dependency graph | `mermaid` fences | Allowed by `pack-format.md` §6, as round 1 |
| Q1 | y | `plan.md` Slice 3, 3.1 | No toolchain or C library named for MicroPython on `wasm32-unknown-unknown` | 3.1 names the default: host `clang` with `wasm-ld`, `-nostdlib`, MicroPython's own string and `libm` sources and GC heap, version recorded. A new stop condition records NLR needing a refused proposal (exception handling) as the blocker |
| Q2 | y | `design.md` §2 Runtime guest `run`; Slices 3 and 4 | How record 1 becomes module `hyalos` in each runtime | `design.md` §2 now states the rule (resolve `from hyalos import task` to record 1's `task` without editing either record), MicroPython's mechanism, and that a runtime unable to do it is blocked and recorded |
| Q3 | y developer-only | prd Assumptions; `context.md` §2; `design.md` R5; Slice 4; ADR-002 | Which switch gives deterministic floats, and in which crate; ADR-002 claimed it could be confined to one profile's builds | Checked `wasmi` 2.0.3's manifest: the Cargo feature `deterministic`. Developer answered 2026-11-25: amend. ADR-007 (accepted) makes it build-wide in spike builds that add a float profile, relying on profiles 1 and 2 refusing floats at load; ADR-002's Status line gains the clause; `context.md` §2, `design.md` R5 and Slice 4 (4.2, stop condition, path) restated |
| Q4 | n | Slice 4, 4.1 | Monty's version unverified | Reader's default adopted: `v1.6.0` if it exists, else the newest release tag, recorded |
| Q5 | n | `design.md` §2 Kernel image spike build; Slice 4 | The layout's memory guards are fixed constants | Reader's default adopted: guards derived from the selected layout, never widened alone; a new Slice 4 stop condition |
| Q6 | n | Slice 4, 4.2 | No rule for the new profile's limits | Reader's default adopted, written into 4.2 (memory to the next MiB above peak, fuel twice the fixture's rounded up to ten million) |
| Q7 | n | `design.md` §2 Runtime guest `run` | `read` has no length query | Fixed buffers sized by the port and recorded; `-2` ends with `error BufferTooSmall`, written into `design.md` §2 |
| Q8 | n | PRD S-04.2.e1 | Two allowed endings | The row now says `finish` of `error AttributeError`, matching `design.md` §2 |
| Q9 | n, re-classed y | `design.md` §2 Triage script; Slice 3, 3.3 | The question text and the digest guest's edge rules were not fixed, so 3.3's byte equality could fail | `design.md` §2's Triage script row now lists every digest rule the script mirrors, including asking with the title bytes verbatim |
| Q10 | n | Slice 3, 3.3 | The digest comparison's command | 3.3 names it: the existing `run` under `hyalos.profile/2`, fixture lines only, seed 0 |
| Q11 | n | `design.md` §2 Spike run command; PRD S-01.3 | Unreadable contract file: message or `BadContract` | `design.md` §2 now separates a file that cannot be read from disk (usage error, no record) from one that reads but is not a contract (`BadContract`) |
| Q12 | n | Slice 4 Verify | Type checker's entry and "unusable" | Slice 4's Verify now defines each outcome |
| Q13 | n | Slices 3 and 4; `profile/src/lib.rs:71` | Both may extend `KNOWN` | Merge-order line under the graph now says the second takes the next number and extends `KNOWN` |
| Q14 | n | Slice 4 Verify | Feature list shorter than Slice 1's | Copied Slice 1's full list |
| Q15 | n | README Open questions | "None" while runtime outcomes are open | Kept "None for planning", with a sentence saying those outcomes are what the spike answers (R1 to R4) |

### Contradictions

| Item | Resolution |
|---|---|
| C1: PRD assumption (monitor and image) against context and design (Monty image only) | As Q3: ADR-007; the PRD's wording was right and stands, the others changed |
| C2: S-04.2.e1 against `design.md` §2 | As Q8: the PRD row changed |
| C3: S-01.3 against the Spike run command row | As Q11: `design.md` §2 clarified; S-01.3 stands |
| C4: README "None" against open risks | As Q15 |
| C5: Slice 1 and Slice 4 feature lists | As Q14 |

### Citations checked

| Citation | Holds |
|---|---|
| `hyalos-os/audit/src/replay.rs:118-124`, `:144-153`, `:378-404`, `:236-246`, `:206`, `:176-179`, `:284-297`, `:300-327`, `:407-438` | yes |
| `hyalos-os/audit/src/record.rs:17`, `:205`, `:256-276`, `:150-189` | yes |
| `hyalos-os/monitor/src/main.rs:182`, `:102-123`, `:155-171`, `:165`, `:903` | yes |
| `hyalos-os/monitor/src/main.rs:682` | partly: the function opens at 682, the variable is read at 683; kept |
| `hyalos-os/monitor/src/main.rs:205-229` | partly per the reader; the author re-checked: `attacks()` spans 205 to 229, holds |
| `hyalos-os/monitor/src/main.rs:459-473` | partly per the reader; the author re-checked: `judge` opens at 459, holds |
| `hyalos-os/monitor/src/kvm.rs:33`, `:29-41` | yes |
| `hyalos-os/monitor/src/kvm.rs:332-353` | partly: trait and impl span 328 to 355; citations in `context.md` and `design.md` changed to `:328-355` |
| `hyalos-os/monitor/src/runner.rs:65-95`, `:400-407` | yes |
| `hyalos-os/guest/src/lib.rs:14-24`, `:107`, `:164-208`, `:177-178` | yes |
| `hyalos-os/ABI.md:24`, `:70-71`, `:29-32`, `:36-38`, `:15-20` | yes |
| `hyalos-os/monitor/src/abi_tests.rs:161-177`, `ring_tests.rs:58` | yes |
| `hyalos-os/profile/src/lib.rs:97-101`, `:87-93`, `:35-71`, `:59-65` | yes |
| `hyalos-os/profile/src/lib.rs:134-146` | partly: a test pinning profile 2 against profile 1, cited as a pattern; fair |
| `hyalos-os/monitor/Cargo.toml:24-31`, `tests/release_switches.rs:63-115` | partly: around the cited lines; kept |
| `hyalos-os/kernel-image/src/main.rs:32-35`, `:233-246`; `crates/xtask/src/hyalos.rs:14`, `:18`; `.github/workflows/ci.yml:284`; `hyalos-os/Cargo.toml:1-5` | yes |
| Fixture, `digest.wasm`, `spikes/73/*`, `docs/adr/0134-…`, `spikes/task-kernel` absent | yes |
| Parent ADR-002 line 30 (`deterministic`) | yes; the author confirmed the feature in `wasmi` 2.0.3's manifest |
| Monty `v1.6.0` | not checked (no network) |

## Round 3 — 2026-11-25

Reader: Haiku (Agent tool, model: haiku, Explore), a new reader on a detached tree of `origin/main`
with the pack copied in. It thought through Slice 4 and Slice 5, checked 27 citations (22 held, 5
partly) and found no code. It flagged the five `mermaid` fences again, re-classed as before. One
item was blocking: the KVM mechanism for a spike profile with other settings.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q-01 | y | `design.md` §2 KVM under a spike profile; Slice 4 4.2 | No mechanism for a spike build to know which profile its kernel image was built for; `on_backend` compares with the compile-time `CURRENT`; no way to build a spike image in a test | `design.md` §2's row now specifies it: a kernel image feature `spike-profile-N`, `HYALOS_KERNEL_PROFILE` read only in a spike build, `on_backend` comparing with that named profile, a test helper beside `built_kernel`, and a mismatch test. Slice 4's 4.2 says how `run.sh` selects both |
| Q-02–Q-06 | n (re-classed) | `design.md` §1, §3, §4, §5; `plan.md` Dependency graph | `mermaid` fences | Allowed by `pack-format.md` §6, as rounds 1 and 2 |
| Q-07 | n | Slice 5, 5.1 | No encoding for `hostile.tsv` | Reader's default adopted: comment lines, then `<id>\t<runtime>\t<escaped line>`, decoded by `hostile.sh`; written into 5.1 |
| Q-08 | n, re-classed y | `design.md` §2; S-06.1.e2 | No data-record buffer size, and no `-2` handling for data | `_hyalos` row: data buffer 16,384 bytes; `read` gives empty bytes for `-2`. Triage row: such a record counts as other without asking, as in the digest. Fixes C-4 too |
| Q-09 | n | Slice 5, 5.1 | Colliding titles differ per runtime | One generated, tagged block per runtime; written into 5.1 |
| Q-10 | n | `design.md` §2 Spike timing | What cold start covers, and in which build | Row now defines `cold_start` and `run_time`, measured by the holder's clock on both backends, no `test-switches` |
| Q-11 | n | `design.md` §2 Runtime guest `run` | Monty's import mechanism | Already answered by the design: recorded as a blocker if Monty has no hook |
| Q-12 | n | Slice 4, 4.1 | `v1.6.0` unverified | Already answered: 4.1 says what to do if the tag is missing |
| Q-13 | n | Slice 4, 4.2 | How peak memory and fuel are read | Reader's default adopted, written into 4.2 |
| Q-14 | n | Slice 4, 4.2 | Layout values | Reader's default adopted: the smallest layout that holds guest and memory, guards derived; written into 4.2 |
| Q-15 | n | Slice 4 Verify | Profile selection, stub location, checker | 4.2 names the variables `run.sh` sets; the stub lives in `spikes/438/monty/` |
| Q-16 | n | Slice 4, 4.3 | The float guest | 4.3 now says it is a WAT module in `spike_tests.rs` |
| Q-17 | n | Shared contract | The evidence test's name | Named `python_committed_evidence_replays_on_both_backends` |
| Q-18 | n, re-classed y | `plan.md`; README rule 5 | Whether a recorded blocker ends a slice `blocked` or `done` | `plan.md` gains a **Recorded blockers** paragraph before the graph: such a stop is a result, the slice ends `done`, and Slice 5 carries it into the findings |
| Q-19 | n | S-01.3; `design.md` §2 | Unreadable file versus not a contract | Already answered in round 2: `design.md` §2 separates the two; S-01.3 and ADR-003 are about record 0 |

### Contradictions

| Item | Resolution |
|---|---|
| C-1: S-01.3 and ADR-003 against the `spike-run` row | Not a contradiction: S-01.3 and ADR-003 concern input record 0; the row's usage error is a file the command cannot read from disk. Unchanged |
| C-2: README rule 5 against Slice 4's recorded blockers | As Q-18 |
| C-3: S-05.1.e2's "findings" against Slice 4's `README.md` | Both: the slice records in `README.md`, Slice 5 carries it into the findings ADR (Recorded blockers paragraph; PRD failure-mode row updated) |
| C-4: digest's `-2` rule missing from the triage script | As Q-08 |
| C-5: ADR-002's described check against the code's `CURRENT` | As Q-01: the mechanism now matches ADR-002's consequence |
| C-6: PRD assumption fallback against ADR-007 | The PRD row now follows ADR-007 |
| C-7: stale text in ADR-002 and ADR-004 | Not edited: both are accepted, marked amended by ADR-007 and ADR-005, which govern |

### Citations checked

| Citation | Holds |
|---|---|
| `hyalos-os/audit/src/replay.rs:118-124`, `:144-153`, `:307-327` | yes |
| `hyalos-os/audit/src/replay.rs:94`, `:206`, `:300`, `:378-404`, `:407-438` | partly: `execute_with` takes a run closure, not a backend; `context.md` §3 corrected |
| `hyalos-os/monitor/src/main.rs:182-189`, `:205-229`, `:903`, `:102-123` | yes |
| `hyalos-os/monitor/src/main.rs:155-171`, `:165` | partly: compares with `CURRENT` (Q-01) |
| `hyalos-os/monitor/src/kvm.rs:29-41`, `:33` | yes |
| `hyalos-os/profile/src/lib.rs:71`, `:97-101`, `:42`, `:134-146` | yes |
| `hyalos-os/monitor/src/runner.rs:65-95` | yes |
| `hyalos-os/monitor/src/runner.rs:633` | partly: `test-switches` only; `measure.sh` uses `spike-run`'s own timing (Q-10) |
| `hyalos-os/audit/src/record.rs:17`, `:22`, `:105`, `:205` | yes |
| `hyalos-os/guest/src/lib.rs:14-24`, `:26-213`, `:128-131` | yes |
| `hyalos-os/ABI.md:70-71`, `:24`, `:29-32`, `:36-38` | yes |
| `hyalos-os/monitor/src/abi_tests.rs:161-177` | yes |
| `hyalos-os/kernel-image/src/main.rs:32-35` | yes |
| `hyalos-os/kernel-image/Cargo.toml:9-14` | partly: no spike feature yet, as expected |
| `hyalos-os/monitor/Cargo.toml:24-31`, `crates/xtask/src/hyalos.rs:14`, `.github/workflows/ci.yml:284` | yes |
| `wasmi` 2.0.3 `deterministic` feature | yes |
| `spikes/task-kernel` absent, `spikes/73/run-python.sh`, `pack-briefs.md` sections, `release_switches.rs:82`, the fixture | yes |

## Round 4 — 2026-11-25

Readers: a, Haiku (Agent tool, model: haiku, Explore), Slices 1, 2 and 3; b, Haiku (Agent tool,
model: haiku, Explore), Slices 4 and 5. Each read a detached tree of `origin/main` (069f31c) with
the pack copied in; the checksum guard was unchanged for both. Reader a checked 28 citations (27
held, 1 partly); reader b checked 27 (25 held, 2 partly). Neither found code; both reported the
five `mermaid` fences under the prompt's literal rule.

Panel: Editor sonnet — 10 findings (3 blocking); Fact-checker sonnet — 3 findings (0 blocking);
Test engineer sonnet — 13 findings (8 blocking); Trust reviewer opus — 10 findings (3 blocking).
Re-check 1: Editor, Fact-checker, Test engineer, Trust reviewer — 42 findings (12 blocking).
Re-check 2: Editor, Fact-checker, Test engineer, Trust reviewer — 33 findings (5 blocking), held
after the second re-check: PV28, PV29, PV30, PT14 (reported again) and PT21. Re-check findings are
renumbered after the first pass's; non-blocking ones are grouped by pass below.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PE1–PE3 | y | editor | `prd.md`, `design.md` | E1: S-04.2 wording, empty-contract outcome, the PRD fallback against ADR-007 | Fixed before the readers |
| PE4–PE10 | n | editor | pack | E1–E6: signing in Non-goals, context §4 features, kernel-image rows, M4 cite, `raw-effect.py` owner, ADR acceptance wording | Adopted before the readers |
| PF1–PF3 | n | fact | `plan.md`, `prd.md` | F1: `main.rs:686`, `ring_tests.rs` pins profile 1 only, `pack-briefs.md:78-85` | Corrected; PF2 led to Slice 1's 1.6 profile 2 pin |
| PV1–PV7, PV9 | y | test | `plan.md` Verify | V1–V6: attest builds, skip gates, `--nocapture`, both env vars, test names, kernel images, NaN bits, `expected.tsv` columns | Fixed before the readers; every Verify block rewritten |
| PV8, PV10–PV13 | n | test | `plan.md` | V1, V4: lint coverage, counts, reproducible builds | Adopted |
| PT1 | y, developer-only | trust | `README.md` | T5: who signs the task identity | Developer (2026-11-25): out of scope; capsules' concern, said in the findings ADR |
| PT2 | y, developer-only | trust | `design.md` §2 | T3: `HYALOS_SPIKE_PROFILE` naming a non-spike profile | Developer (2026-11-25): refused, usage error, no record |
| PT3 | y, developer-only | trust | `design.md` §2 | T1: who names a KVM image's profile | Developer (2026-11-25): the image reports it after a marker in its bytes; `HYALOS_KERNEL_PROFILE` (round 3, Q-01) dropped |
| PT4–PT10 | n | trust | pack | T1–T4: stand-ins, failure rows, owners | Adopted or recorded in `design.md` §7 |
| PE11–PE21 | n | editor | pack | Re-check 1, E2: restatements the first fixes left | Adopted before re-check 2 |
| PF4–PF10 | n | fact | pack | Re-check 1, F1–F4: drifted lines | Corrected |
| PV14–PV27 | y (10), n (4) | test | `plan.md` | Re-check 1, V1–V6: profile 2 pin, 4.2's limits test, the canonical NaN, `kernel.sh`, seeds 0 and 7, same-path builds, test names | Fixed before re-check 2 |
| PT11 | y, developer-only | trust | `plan.md` Shared contract | Re-check 1, T1: which upstream repositories, and how the SHA is anchored | Developer (2026-11-25): the pack names `github.com/micropython/micropython` and `github.com/pydantic/monty` (the executor confirms and records them); the first fetch is trusted and the full SHA reviewed in the PR; no signed-tag check |
| PT14 | y, developer-only, held | trust | `plan.md` Recorded blockers | Re-check 1 and 2, T3: a KVM `Timeout` is host-dependent but the replay test re-runs it | Developer (2026-11-25): the case is listed `process`, with a `process-only:` line in `README.md` and a `MEASUREMENTS.md` row; CI never replays a wall-clock outcome |
| PT12, PT13, PT15–PT20 | n | trust | pack | Re-check 1, T1–T4 | Adopted (`kernel.sh`, release-switch proof, versions recorded) |
| PV28 | y, held | test | `plan.md` Shared contract | Re-check 2, V5: `expected.tsv` was written by the run it checks | Fixed: no script writes `expected.tsv`; the executor writes PRD cases from the example rows and hostile cases after review; Verify adds `git diff --exit-code` on the evidence |
| PV29 | y, held | test | `plan.md` Shared contract | Re-check 2, V2: nothing ties a `process` case to a recorded reason | Fixed: the test fails when a `process` case has no `process-only:` line in `spikes/438/README.md` |
| PV30 | y, held | test | `plan.md` Slice 1 | Re-check 2, V6: 1.5 had no check | Fixed: test `python_1_5_spike_task_equals_record_task`, with a `--list` count in Verify |
| PT21 | y, developer-only, held | trust | `plan.md` Recorded blockers | Re-check 2, T3: a failed upstream fetch, result or block | Developer (2026-11-25): a hard stop (an environment fault); a script may reuse a fetched tree whose commit equals the pinned SHA, so Verify runs offline |
| PE22–PE33, PF11–PF14, PV31–PV36, PT22–PT26 | n | all | pack | Re-check 2, non-blocking | PE23, PE25 and PE30 are aQ1, bQ6 and bK1 below; the rest stand as recorded in the re-check reports, to be read again by the Editor pass |
| aF1–aF5, bQ1–bQ5 | n | reader | `design.md` §1, §3–§5, `plan.md` graph | The five `mermaid` fences read as code | Re-classed non-blocking: `pack-format.md` §6 allows `mermaid` fences, and each holds a diagram |
| aQ1 | y | editor | `prd.md` S-01.3, `design.md` §2 | A missing or unreadable contract: `BadContract` or a usage error | Fixed: `design.md` follows S-01.3 and ADR-003 again: an unreadable, empty or invalid contract ends the run `BadContract` before the guest loads; only the SDK, script and data files give usage errors. Checklist line proposed for editor: **E7** When one input's refusal (a reason, a usage error, an exit) is stated in more than one file, the outcomes must match; a difference is blocking, since an executor builds one and tests the other |
| aQ2 | y | editor | `README.md` rule 5, `plan.md` Recorded blockers | Rule 5 makes every stop `blocked`; the plan ends recorded results `done` | Fixed: rule 5 names the exception. Checklist line proposed for editor: **E8** Check each README "How to execute" rule against `plan.md`'s own rules; a plan rule that overrides one needs the README to name the exception |
| bQ6 | y | test | `plan.md` 4.2 | No command measures the guest's peak memory | Fixed: peak memory is the smallest whole MiB of `max_memory` at which the seed-0 process-backend run ends with a result, found under a temporary, uncommitted `KNOWN` entry (also answers bQ7). Checklist line proposed for test: **V7** For each measured figure a criterion depends on, name the command that measures it; flag a figure no command or slice produces |
| aQ3 | n | reader | `prd.md` S-01.3.e2 | No command produces zero input records | Default adopted: a test calls `execute_with` with no inputs on both backends |
| aQ4 | n | test | `plan.md` Slice 1 Verify | Repository-wide checks left out | Pack stands: `hyalos-os/` is outside the main workspace; `context.md` §4 names when the CLAUDE.md checks apply. Names PV35 |
| aQ5–aQ11, aQ13 | n | reader | `plan.md`, `design.md` | Base commit, timing source, generator output form, digest cap, seed 7, port buffers, profile choice | Defaults adopted as the readers gave them; the base commit is the slice's own base, as 1.6 says |
| bQ8 | n | trust | `plan.md` 3.1, 4.1 | No upstream URL | Answered by PT11 |
| bQ9, bQ10, bQ13, bQ16, bQ17, bQ20 | n | reader | `plan.md` | README line authorship, Monty import, layout numbers, case grouping, table form, float test inputs | Defaults adopted as the reader gave them |
| bQ11 | n | editor | `prd.md` S-04.4, `design.md` §7 | No import check for Monty | Pack stands: `BadImport` refuses any extra import at load, on both backends |
| bQ14 | n | editor | `plan.md` 5.1 | `hostile.sh` reads and writes `expected.tsv` | Fixed with PV28: it prints outcomes; the executor writes the lines |
| bQ15 | n | reader | `plan.md` 5.1 | A hash that cannot be collided | Default adopted: record "no collision found" and run 200 same-length titles |
| bQ18 | n | test | `plan.md` 1.3 | CI's change filter skips an evidence-only change | Fixed: 1.3 adds `spikes/438/**` to the `hyalos-os` filter. Checklist line proposed for test: **V8** For a check that runs only in CI, confirm the job's path filter matches every file the check reads |
| bQ19 | n | editor | `prd.md` S-06.1.e2 | 16 KiB with its prefix collapses the two rows | Fixed: 16,384 and 16,385 bytes, in the PRD and Slice 5 |

### Contradictions

| Item | Resolution |
|---|---|
| aC1 (editor): `prd.md` S-01.3 against `design.md` §2 on an unreadable contract | `design.md` changed (aQ1); names E7 |
| aC2 (reader): S-01.3.e2 has no entry point | aQ3 |
| aC3 (reader): ADR-002 describes the KVM comparison as current | Not edited: ADR-002 is accepted; `design.md` §2 gives the mechanism and its slice |
| aC4 (editor): README rule 5 against Recorded blockers | README changed (aQ2); names E8 |
| aC5 (test): repository-wide checks | aQ4 |
| aC6, bK5 (reader): ADR-004 says `return` | Not edited: amended by ADR-005, which governs |
| aC7, bK6 (reader): ADR-002 says the float setting is confined | Not edited: amended by ADR-007, which governs |
| aC8 (reader): `ABI.md` one line per record against whole-file records | Intended; ADR-0136 states the exception (1.2) |
| bK1 (editor): the PRD assumption on profile 1 and 2 fuel against Slice 4's stop | `prd.md` changed: a hard stop under ADR-007, the slice ends `blocked`; names E8 |
| bK2 (editor): `design.md` §7 says S-04.4 checks each runtime | bQ11 |
| bK3 (test): the `-hostile-` count formula | `plan.md` Slice 5 Verify changed: one line per case and runtime it applies to; names V7 |
| bK4 (editor): `expected.tsv` both input and output of `hostile.sh` | PV28 |
| bK7 (reader): the base commit in 1.6 against `context.md` | 1.6 means the slice's own base commit; `context.md` records when the citations were checked |

### Citations checked

| Citation | Holds |
|---|---|
| a: `hyalos-os/audit/src/replay.rs:118-124`, `:144-153` | yes |
| a: `hyalos-os/monitor/src/main.rs:182`, `:155-171`, `:165`, `:205-229`, `:686`, `:102-123` | yes |
| a: `hyalos-os/monitor/src/kvm.rs:33` | partly: the constant holds; the refusal was not traced |
| a: `hyalos-os/monitor/src/kvm.rs:29-41`, `hyalos-os/audit/src/record.rs:17`, `:205` | yes |
| a: `hyalos-os/profile/src/lib.rs:42`, `:71`, `:97`, `crates/xtask/src/hyalos.rs:14`, `:18` | yes |
| a: `abi_tests.rs:161-177`, `ring_tests.rs:12-14`, `guest/src/lib.rs:28`, `:140-150`, `:164-208` | yes |
| a: `ABI.md:70-71`, `release_switches.rs:81-99`, `ci.yml:284`, `runner.rs:65-95`, `monitor/Cargo.toml:26-31`, `kernel-image/src/main.rs:233-246` | yes |
| a: `spikes/task-kernel` absent, ADR-0136 free, `spikes/73/run-python.sh`, `guest/digest.wasm`, the fixture | yes |
| b: `main.rs:182`, `:165`, `:686`, `:102-123`, `:205-229`, `kvm.rs:33`, `:29-41` | yes |
| b: `profile/src/lib.rs:42`, `:71`, `guest/src/lib.rs:28`, `:128-131`, `profile_tests.rs:114-121`, `ring_tests.rs:12-14` | yes |
| b: `abi_tests.rs:626-640` | partly: the `ABI.md` coverage check; `every_reason` is at `:103` |
| b: `runner.rs:65`, `:633`, `release_switches.rs:82`, `kernel-image/src/main.rs:233-246`, `ABI.md:70-71`, `record.rs:17`, `replay.rs:94`, `:378`, `xtask/src/hyalos.rs:14`, `ci.yml:284`, `:68` | yes |
| b: `pack-briefs.md:78-85` | partly: the table holds; it names no repository URL (PT11) |
| b: ADR-0134, `spikes/73/run-python.sh` | yes |

## Round 5 — 2026-11-25

Readers: a, Haiku (Agent tool, model: haiku, Explore), Slices 1, 3 and 4; b, Haiku (Agent tool,
model: haiku, Explore), Slice 5. Each read a detached tree of `origin/main` (069f31c) with the pack
copied in; every checksum guard was unchanged. Reader a's first report checked no citations (its
reads of the repository were refused), so a new reader a read the group again and its report is the
one recorded: 29 citations checked, 27 held, 2 partly. Reader b checked 23: 20 held, 2 partly, 1 not
checked (the commit, its `git` call refused). Neither found code; both again reported the five
`mermaid` fences under the prompt's literal rule.

Panel: Editor pass on round 4's diff, Editor sonnet — 8 findings (2 blocking), all fixed before the
readers.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PE34 | y | editor | `README.md` rule 5, `plan.md` Recorded blockers | E1: which "record" stops end `done` | Fixed before the readers; tightened again under aQ-Q1 below |
| PE35 | y | editor | `plan.md` Recorded blockers | E4: a `Timeout` needed a `MEASUREMENTS.md` row before Slice 5 makes the file | Fixed: Slice 5 writes the row from the `process-only:` line |
| PE36 | n | editor | `plan.md` Slices 3, 4 Files | E4: `raw-effect.py` has two creators | Fixed: Slice 3 creates it or uses Slice 4's |
| PE37 | n | editor | `design.md` §2 `spike-task` | E2: its contract refusal after the `spike-run` change | Fixed: an unreadable, empty or invalid contract prints `BadContract` |
| PE38 | n | editor | `plan.md` Slice 3 Files | E3: `kernel.sh` missing from the 3.4 branch | Added |
| PE39 | n | editor | `plan.md` Slice 5 Files | E3: `README.md` for a hostile case's `process-only:` line | Added |
| PE40 | n | editor | `design.md` P3 | E6: cites S-06.3 for what only 5.3 states | Cites 5.3 |
| PE41 | n | editor | `plan.md` 5.2 | E6: a process-only runtime's KVM row | Gives the `process-only:` reason in place of figures |
| aQ-01–aQ-05, bF-1–bF-5 | n | reader | `design.md`, `plan.md` | The five `mermaid` fences | Re-classed non-blocking, as in round 4: `pack-format.md` §6 allows them |
| bS5-1 | y | editor | `plan.md` 5.1 | No method for Monty's colliding titles | Fixed: `collide.py` reimplements each runtime's hash from its pinned source, cited in `README.md`; a hash it cannot reproduce gives 200 same-length titles and a recorded result. Names E7 |
| bS5-2 | y | test | `plan.md` 5.1, `prd.md` S-06.1.e3 | "Fuel recorded" for an `OutOfFuel` case has no source | Fixed: the fuel recorded is the profile's `fuel`, which the run used up. Names V7 |
| aQ-09 | n | editor | `design.md` §2 `spike-run` | "Becomes record 0" against "ends `BadContract`" | Fixed: an invalid contract that reads is input record 0 of a record that ends `BadContract`; an unreadable or empty one writes no record. Names E7 |
| aQ-11 | n | test | `plan.md` Slice 3 Verify | No exemption when MicroPython is recorded blocked | Added, as Slice 4 has. Names V7 |
| aQ-12 | n | fact | `prd.md` assumption, `plan.md` 3.4 | A profile cannot raise the 1 MiB guest limit | Fixed: both say a guest over 1 MiB needs a spike-only KVM layout (`kvm.rs:33`). Checklist line proposed for fact: **F8** For each limit the pack says a setting can change, open the code that holds the limit and confirm the setting reaches it |
| aQ-14 | n | editor | `design.md` §2, `plan.md` Slices 3, 4 | A runtime that cannot load record 1 as `hyalos` | Fixed: a stop condition in each slice, a recorded result |
| aQ-20 | n | editor | `plan.md` | Three forms of the `process-only:` line | Fixed: one form, `process-only: <runtime or case id> — <reason>`, with what each reason names |
| aQ-06–aQ-08, aQ-10, aQ-13, aQ-15–aQ-19 | n | reader | `plan.md`, `design.md` | Canonical check, `Host` field, record limits, CI filter (1.3 adds it), buffer sizes, probe values, marker bytes, type checker | Defaults adopted as the reader gave them |
| bS5-3–bS5-10 | n | reader | `plan.md` Slice 5 | Case lines, hash algorithm, contracts per case, decision rule, medians, `process` lines, ADR number, brief placement | Defaults adopted as the reader gave them |

### Contradictions

| Item | Resolution |
|---|---|
| aC-1 (fact): a profile against `GUEST_MAX` | aQ-12 |
| aC-2 (editor): the `process-only:` line's contents | aQ-20 |
| aC-3, aC-4 (reader): ADR-004 and ADR-002 text | Not edited: amended by ADR-005 and ADR-007, which govern |
| aC-5 (test): the Slice 3 Verify exemption | aQ-11 |
| bK-1 (editor): S-06.1 "a result or a refusal" against a KVM `Timeout` | `plan.md` 5.1 says a `Timeout` is a refusal with reason `Timeout`. Names E7 |
| bK-2 (test): `OutOfFuel` fuel | bS5-2 |
| bK-3 (editor): Slice 5's hang stop against a `Timeout` | The stop now reads "run on past its limit with no reason at all" |

### Citations checked

| Citation | Holds |
|---|---|
| a: `replay.rs:118-124`, `:144-153`, `:378-404`, `:407-438`, `:94`, `:206` | yes |
| a: `main.rs:182-189`, `:155-171`, `:165`, `:686`, `:205-229` | yes |
| a: `kvm.rs:33` | partly: the constant; the refusal site not read |
| a: `kvm.rs:29-41`, `:328-355`, `record.rs:17`, `:21-52`, `:150-189`, `runner.rs:65-95` | yes |
| a: `profile/src/lib.rs:35-71`, `:87-93`, `:97-101`, `guest/src/lib.rs:28`, `:164-208` | yes |
| a: `ABI.md:70-71`, `:15-20`, `:29-32`, `:36-38`, `ring_tests.rs:12-14`, `xtask/src/hyalos.rs:14`, `:18` | yes |
| a: `ci.yml:68` | partly: the filter does not match `spikes/438/` yet; 1.3 adds it |
| a: ADR-0134, `pack-briefs.md:78-85`, the fixture, `spikes/73` | yes |
| b: `main.rs:686`, `:205-229`, `:165`, `:182-189`, `runner.rs:633`, `ABI.md:70-71` | yes |
| b: `main.rs:459-473` | partly: `judge` at 459-470; the list is at 205-226 |
| b: `guest/src/lib.rs:28`, `:128-131`, `:164-208`, `kvm.rs:29-41`, `:328-355`, `abi_tests.rs:161-177`, `profile_tests.rs:114-121` | yes |
| b: `replay.rs:94`, `profile/src/lib.rs:71`, `kernel-image/src/main.rs:233-246`, `pack-briefs.md` sections, ADR-0136 free, `spikes/438` and `spike_evidence_tests.rs` absent | yes |
| b: `ci.yml:68` | partly: as for reader a |
| b: the base commit 069f31c | not checked: the reader's `git` call was refused |

## Round 6 — 2026-11-25

Readers: a, Haiku (Agent tool, model: haiku, Explore), Slices 2, 3 and 4; b, Haiku (Agent tool,
model: haiku, Explore), Slice 5. Each read a detached tree of `origin/main` (069f31c) with the pack
copied in; both checksum guards were unchanged. Reader a checked 36 citations (34 held, 2 partly);
reader b checked 30 (29 held, 1 partly). Neither found code; both again reported the five
`mermaid` fences under the prompt's literal rule.

Panel: Editor pass on round 5's diff, Editor sonnet — 7 findings (1 blocking), all fixed before the
readers.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PE42 | y | editor | `design.md` §2 `spike-run` | E2: a `BadContract` run was said to write a record; elsewhere a refused run writes none | Fixed before the readers: it is a refusal and writes no record |
| PE43 | n | editor | `prd.md` S-01.3, `context.md` glossary | E5: `BadContract`'s scope narrower than its example rows | The statement and glossary now say missing, unreadable, empty or not a contract; ADR-003 is accepted and not edited, and its "missing or unreadable" is a subset |
| PE44 | n | editor | `design.md` §2, `plan.md` 3.4, `context.md` | E2: the layout trigger left out a guest over 1 MiB | Each now names it |
| PE45 | n | editor | `plan.md` Recorded blockers, `README.md` rule 5 | E2: "ends with" the marker words | Now "contains"; the README says the same |
| PE46 | n | editor | `plan.md` Slice 2 Verify | E1: stale reason `check.sh` is outside CI | Reworded |
| PE47 | n | editor | `prd.md` S-06.1.e3 | E2: the collision fallback | The row names it |
| PE48 | n | editor | `plan.md` Slice 5 | E1: Verify hard-coded `ADR-008` | The file name always has `-findings-`, and Verify finds it by that |
| aF1–aF5, bQ1–bQ5 | n | reader | `design.md`, `plan.md` | The five `mermaid` fences | Re-classed non-blocking, as in rounds 4 and 5: `pack-format.md` §6 allows them |
| aQ2a, aQ2b, aQ2e | n | reader | `design.md` §2 Triage script | Malformed records, `parse_number`'s rules, the listing layout | Defaults adopted as the reader gave them: mirror the digest (`hyalos-os/guest/src/lib.rs:63-76`, `:164-208`) byte for byte, 3.3 proves it |
| aQ2c | n | reader | `plan.md` 2.1 | A trailing newline on a contract file | Default adopted: exact canonical bytes, no newline |
| aQ2d | n | reader | ADR-004 against `design.md` §2 | `finish` with or without text | `design.md` governs; ADR-004 is amended by ADR-005 |
| aQ3a–aQ3c, aQ4a–aQ4e | n | reader | `plan.md` Slices 3, 4 | Buffer sizes, module loading, shared files, layout values, peak memory, `getrandom`, the type checker, `KNOWN`'s length | Defaults adopted as the reader gave them; the shared files follow the merge note under the graph |
| bQ6–bQ20 | n | reader | `plan.md` Slice 5 | Case grouping, `collide.py` output, fuel, medians, retro, invocation, reach fields, untracked records, the count, the ADR's acceptance, escapes, where weaknesses are written, the brief line, the test's path, the layout | Defaults adopted as the reader gave them; README rule 8 governs the retro |

### Contradictions

| Item | Resolution |
|---|---|
| aC1 (reader): the monitor's record limit against the port's buffers | No conflict: the monitor passes every byte; the guest's own buffer may refuse it, recorded as `BufferTooSmall` |
| aC2 (reader): ADR-004's `finish()` | aQ2d |
| aC3 (reader): `ci.yml:49` treats `spikes/` as docs | The `hyalos-os` job gates only on its own filter (`ci.yml:236`), which 1.3 widens |
| aC4 (reader): M1 against S-05.1's process-only case | S-05.1 governs; M1 is a goal |
| aC5 (reader): parallel slices and shared files | aQ3c |
| bC1 (reader): fuel for `OutOfFuel` | The profile's `fuel`, as 5.1 says |
| bC2 (reader): `git diff` misses untracked records | Pack stands: the check runs on the committed tree, and a new record there is a `*.json` no case lists, which the replay test fails on |
| bC3 (reader): "add it to nothing else" against README rule 8 | README rule 8 governs |
| bC4, bC6 (reader): medians and the hostile count | Defaults adopted as the reader gave them |
| bC5 (reader): the ADR's accepted line in Verify | The slice is done only after the developer's approval, as 5.3 says |

### Citations checked

| Citation | Holds |
|---|---|
| a: `replay.rs:118-124`, `:144-153`, `:378`, `main.rs:182-189`, `:686`, `:165`, `:205-229`, `:102-123`, `:903`, `:704` | yes |
| a: `kvm.rs:33`, `:29-41`, `profile/src/lib.rs:71`, `:42`, `record.rs:17`, `runner.rs:65` | yes |
| a: `abi_tests.rs:626-640` | partly: `every_reason` is at `:103` |
| a: `abi_tests.rs:161-177`, `ring_tests.rs:12-14`, `:58`, `guest/src/lib.rs:28`, `:164-208`, `:14-24`, `:107-108` | yes |
| a: `ci.yml:68` | partly: 1.3 adds `spikes/438/**` |
| a: `ci.yml:284`, `xtask/src/hyalos.rs:14`, `:18`, `kernel-image/src/main.rs:32-35`, `profile_tests.rs:152-155`, `release_switches.rs:82`, `ABI.md:70-71` | yes |
| a: the fixture, `spikes/` contents, ADR-0134, ADR-0136 free, `pack-briefs.md:78-85` | yes |
| b: `replay.rs:94`, `:118-124`, `:144-153`, `:378`, `main.rs:182-189`, `:686`, `:155-171`, `:205-229`, `:459-473`, `:903` | yes |
| b: `kvm.rs:33`, `:29-41`, `guest/src/lib.rs:28`, `:128-131`, `:32`, `runner.rs:65-95`, `:633`, `profile/src/lib.rs:71` | yes |
| b: `profile_tests.rs:114-121`, `:152-154`, `ABI.md:70-71`, `ci.yml:284-292`, `xtask/src/hyalos.rs:14`, `:18`, `release_switches.rs:82` | yes |
| b: `pack-briefs.md:78-85` | partly: the table is at about 80-85 |
| b: ADR-0136 free, `spikes/task-kernel` absent, `hyalos-os/Cargo.toml:5-6`, `ring_tests.rs:58` | yes |

## Result

Pass after 6 round(s) (3 in this run).

### Checklist proposals

- aQ1 → editor.md: **E7** When one input's refusal (a reason, a usage error, an exit) is stated in more than one file, the outcomes must match; a difference is blocking, since an executor builds one and tests the other
- aQ2 → editor.md: **E8** Check each README "How to execute" rule against `plan.md`'s own rules; a plan rule that overrides one needs the README to name the exception
- bQ6 → test-engineer.md: **V7** For each measured figure a criterion depends on, name the command that measures it; flag a figure no command or slice produces
- bQ18 → test-engineer.md: **V8** For a check that runs only in CI, confirm the job's path filter matches every file the check reads
- aQ-12 → fact-checker.md: **F8** For each limit the pack says a setting can change, open the code that holds the limit and confirm the setting reaches it
