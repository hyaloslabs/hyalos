# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 18 |
| **Result** | Pass after 18 rounds (3 in this run). |

## Round 1 — 2026-11-25

Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 31 citations, 26 held and 5 partly; reader b (Haiku, Slices 4, 5, 6) checked 30, 23 held, 6 partly and 1 did not (`SOURCE_CAP`); reader c (Haiku, Slices 7, 8, 9) checked 33, 30 held, 2 partly and 1 did not (`ci.yml` has no `python3`); reader d (Haiku, Slices 10, 11, 12) checked 36, 29 held, 5 partly and 1 did not (`release_switches.rs:6-8`). Every reader reported the `mermaid` and layout `text` fences as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

Review panel before the readers: Fact-checker (sonnet) 3 findings, none blocking; Test engineer (sonnet) 12 findings, 4 blocking; Editor (sonnet) 12 findings, 2 blocking; Trust reviewer (opus) 9 findings, 5 blocking, all developer-only. Re-check 1 (all four roles with the fix diff): Editor 7 (1 blocking), Test engineer 8 (2 blocking), Fact-checker 10 (none blocking), Trust reviewer 9 (6 blocking, developer-only). Re-check 2: Editor 6 (none blocking), Test engineer 9 (3 blocking), Fact-checker 4 (1 blocking), Trust reviewer 10 (5 blocking, developer-only). The developer answered every developer-only item (ADR-023 to ADR-026); every finding was fixed in the pack before the readers ran.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PT1 | y, developer-only | trust | plan.md Slice 10 | Live run's customer, public effects, scripts, and no version 5 switch for `live` (with PE1) | Answered 2026-11-25: ADR-023 (`live --version 5` in a `test-trust` build, the developer as customer and operator, two probe scripts) |
| PT3 | y, developer-only | trust | plan.md Slice 10 | No proof the figures came from the real provider; key from the notary's stdout | Answered 2026-11-25: ADR-023, ADR-024 (`attest check`), ADR-025 (`notary public`) |
| PT4 | y, developer-only | trust | prd.md S-06.5.e2 | No way to sign a free-text request, nor its gate | Answered 2026-11-25: ADR-023 (`free-text-notary` behind its own feature), ADR-025, ADR-026 (its key) |
| PT5 | y, developer-only | trust | plan.md Slice 9 | Golden version 5 record has no fixed keys | Answered 2026-11-25: ADR-023, ADR-025, ADR-026 (`golden/v5/` with both halves) |
| PT6 | y, developer-only | trust | design.md §2.4, §2.8, §2.9 | New audit failures name no exit code | Answered 2026-11-25: ADR-023 (refused exit 1; pre-pin inconclusive exit 2) |
| PE1, PE2 | y | editor | plan.md Slices 1, 3, 10; design.md §1 | No slice creates the version 5 switch; `run.sh` described two ways | Fixed: ADR-023's option in Slice 10; design `run.sh` row matches Slice 1 |
| PV1, PV2, PV3, PV7 | y | test | plan.md Slices 8, 10, 11, 12 | Greps pass without the figure; counts with one test of slack | Fixed: anchored rows and tables, recomputed counts |
| Re-check 1 PE1, PV1, PV2 | y | editor, test | plan.md Slices 6, 7, 9 | S-06.5.e2 proven in two slices; python3-absent check passes on any failure; e2 cause not asserted | Fixed: e2 wholly in Slice 9; fixed panic message counted; template message asserted |
| Re-check 1 PT1 to PT6 | y, developer-only | trust | plan.md Slices 9, 10 | Notary key, test keys, measured build, order after `live`, S-10.3 before the pin | Answered 2026-11-25: ADR-025 |
| Re-check 2 PV1, PF1, PV2, PV3 | y | test, fact | plan.md Slices 6, 9, 10 | `free-text-notary` not built before Slice 10's tests or in the spike CI job; refusals indistinct; exit 2 cannot show no network call | Fixed: both CI jobs build it; `refused: root` and `refused: notary` lines; S-10.3 tests assert fixed messages and read files only |
| Re-check 2 PT1 to PT5 | y, developer-only | trust | plan.md Slices 9, 10 | Public key files, `free-text-notary`'s key, the notary's state and setup, attempts | Answered 2026-11-25: ADR-026 |
| aQ1 | y | editor | plan.md Slice 3; design.md §2.4 | `task` cannot print the `task_v5` identity before the pin | Fixed: `task --version 5`, accepted only in a `test-trust` build, removed at the pin. Checklist line proposed for editor: E7 For each command a criterion runs before a version switch, check that the command can select that version. |
| bS6-1 | y, developer-only | reader | design.md §2.7; ADR-017 | The default instruction's wording is never given | Answered 2026-11-25: ADR-027, `Answer the question with one value of the given type.` |
| bS6-2 | y | fact | plan.md Slice 6 | The canary literal is tied to `hidden_header.py` by tests the slice would break | Fixed: Slice 6 changes the probe with the canary, and runs both equality tests. Checklist line proposed for fact: F8 For every literal a slice changes, find every test or file held equal to it. |
| cQ-01 | y | reader | design.md §2.10 | A contract `name` may be a Python keyword | Fixed: the generator appends `_` to keyword names; `Contract::parse` unchanged |
| cQ-02 | y | test | plan.md Slice 8 | No method to read the peak heap | Fixed: the process backend reads the static heap's high-water mark after the run; the KVM leg's byte-identical record shows the same state. Checklist line proposed for test: V7 For each measured figure a criterion needs, name the command or reading that produces it. |
| cQ-03 | y | trust | plan.md Slice 9 | The `standin --answer` record for S-10.3.e3 has no source | Fixed: generated once and committed as `golden/v5/standin-answer.record`. Checklist line proposed for trust: T7 For each committed signed fixture, name who signs it and with which committed key. |
| dQ1, dQ2 | y, developer-only | reader | design.md §2.2; plan.md Slices 10, 11 | Triage's limit before the measurement; a ceiling below 200 breaks the largest input | Answered 2026-11-25: ADR-027 (200 before; triage's accepts shrink to the ceiling in Slice 11) |
| dQ3 | y | editor | plan.md Slice 12 | Deleting the spike tests drops S-01.2.e2's only proof | Fixed: Slice 12 adds a default-build replay test of the v5 triage record. Checklist line proposed for editor: E8 For each file a slice deletes, check that every criterion it proved is proven elsewhere afterwards. |
| aF1 to aF12, bF1 to bF12, cQ-04 to cQ-07, dF1 to dF12 | n | reader | design.md, plan.md, context.md fences | `mermaid` and layout `text` fences read as code | Re-classed non-blocking: pack-format §6 allows `mermaid` for diagrams and `text` for layouts. The two `text` fences holding shell commands became a `bash` fence (context §4) and an inline list (design, Modifier: performance) |
| aQ2 to aQ15, bS6-3 to bS6-8, bS5-1 to bS5-4, bS4-1 to bS4-3, cQ-08 to cQ-20, dQ4 to dQ18 | n | reader | various | Defaults the readers would choose | Adopted into the pack where they set a value an executor needs (the `ReadError` split, the `RELEASE` digest input, typed answer output, legacy grants, output paths, golden names, test contract); the rest stand as written |

### Contradictions

| Item | Resolution |
|---|---|
| aK1 (editor): design §2.6 pins the abi/2 digest now; §1, ADR-012 and Slice 2 do not | design §2.6 changed: pinned in Slice 11 |
| aK2 (editor): S-03.2.e2 against `task`'s version | Fixed with aQ1 |
| aK3, dK2 (fact): `release_switches.rs:6-8` cited as a `test-trust` column; the file's switch is `SWITCHES` on `test-switches` at `:23` | Citations changed: a new `TRUST` constant beside `SWITCHES`. Checklist line proposed for fact: F9 For every cited line that a contract calls a named constant, quote the constant's name and condition. |
| aK4 (reader): the version 4 input rule names `task`, which writes no record | Slice 2 states `task` refuses the same records |
| aK5, cX-05 (editor): a v5 record with `abi` removed is exit 1 in Slice 3, exit 2 pre-pin in design | Slice 3 changed: exit 1 in `test-trust` builds; a default build gives exit 2 first |
| aK6 (editor): S-02.4.e1 needs a digest pinned only in Slice 11 | prd row changed to name Slice 11 for that half |
| bK1 (editor): `attest_check::check` taking the version is conditional | Made unconditional |
| bK2 (test): Slice 6's free-text build expected to pass and to fail | Notes changed |
| bK3 (fact): canary against the probe tests | Fixed with bS6-2 |
| bK4 (editor): five template copies listed two ways | Slice 6 changed to context §3's list |
| bK5 (editor): S-07.3.e1's audit side | Slice 5 names Slice 9 |
| bK6 (fact): `SOURCE_CAP` named as existing | Now a new constant, 16,384 |
| bK7 (editor): `InputLimit` for too few code records only in design | Added to Slice 4 and as a prd example row |
| cX-01 (editor): S-09.3.e2 described two ways in Slice 7 | Contracts changed to match the prd |
| cX-02 (reader): plan says design §2.10 is silent on non-UTF-8 records | Note removed |
| cX-03 (editor): triage `questions` has no source | Fixed with ADR-027 |
| cX-04 (reader): Slice 8's CI figure and the same-PR progress file | Slice 8 states a second commit after the first CI run |
| cX-06, cX-07 (reader): spike generator's CLI; ADR-014's stale body | No change: noted for the executor; ADR-014's Status records ADR-017 |
| dK1 (fact): ticket count read from `notary report`, which prints none | Changed to `notary tickets --dir` |
| dK3 (editor): 30 s and 35 s for one limit | Slice 11 names both, as the prd row does |
| dK4, dK8 (editor): literal 200 and 201 in prd rows | prd rows name the provisional and the fixed ceiling; ADR-027 for the triage input |
| dK5, dK7 (editor): triage L's source; "left the repository" | Fixed with ADR-027 |
| dK6 (editor): S-01.2.e2 unproven after Slice 12 | Fixed with dQ3 |
| dK9 (fact): `attest-check` is a dev-dependency of verify | context §2 D changed |

### Citations checked

| Citation | Holds |
|---|---|
| a `hyalos-os/monitor/src/main.rs:199-206` | yes |
| a `hyalos-os/kernel-image/Cargo.toml:11` | yes |
| a `hyalos-os/audit/src/record.rs:17` | yes |
| a `hyalos-os/monitor/src/abi_tests.rs:12-20` | yes |
| a `hyalos-os/monitor/src/main.rs:926-933` | partly |
| a `hyalos-os/monitor/tests/release_switches.rs:6-8` | partly |
| a `hyalos-os/monitor/src/spike.rs:84` | partly |
| a `hyalos-os/verify/src/main.rs:148-155` | yes |
| a `hyalos-os/monitor/src/main.rs:1002-1030` | partly |
| a `hyalos-os/monitor/src/abi_tests.rs:597` | partly |
| b `hyalos-os/audit/src/replay.rs:139-174` | partly |
| b `hyalos-os/audit/src/replay.rs:99-102` | partly |
| b `hyalos-os/attest/src/bin/canary.rs:44` | partly |
| b `hyalos-os/attest/src/bin/notary.rs:745-754` | yes |
| b `.github/workflows/ci.yml:357-360` | partly |
| b `SOURCE_CAP` | no |
| b `hyalos-os/guest/src/lib.rs:35-38` | partly |
| c `hyalos-os/monitor/src/runner.rs:64-105` | partly |
| c `docs/plans/hyalos-kernel-python/decisions/ADR-008-…:115-118` | yes |
| c `spikes/438/generate/generate.py:1-80` | partly |
| c `ci.yml` `python3` in the `hyalos-os` job | no |
| d `hyalos-os/monitor/src/live.rs:21-22` | partly |
| d `hyalos-os/attest/src/bin/notary.rs:1175, :1209` | partly |
| d `hyalos-os/verify/Cargo.toml:17` | partly |
| d `hyalos-os/monitor/src/main.rs:138` | partly |
| d `hyalos-os/monitor/tests/release_switches.rs:6-8` | no |

The other 104 citations the readers checked held.

## Round 2 — 2026-11-25

Four new readers on new trees of `origin/main` (`bf09657`), over the slices round 1's fixes touched: reader a (Haiku, Slices 2, 3, 4) checked 24 citations, 20 held and 4 partly; reader b (Haiku, Slices 5, 6, 7) checked 31, 29 held and 2 partly; reader c (Haiku, Slices 8, 9, 10) checked 36, 32 held and 4 partly; reader d (Haiku, Slices 11, 12) checked 23, 19 held, 1 partly and 3 did not (two name files later slices create; one is `scope.toml`, dK5). Every reader again reported the `mermaid`, layout `text` and context §4 `bash` fences on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| aQ4 | y | editor | plan.md Slices 2, 3 | Slice 3's probe line passes the line-form fixture Slice 2 refuses | Fixed: the probe frames the fixture with `frame --lines` first; Slice 3's Contracts line and grep match. Checklist line proposed for editor: E9 For each input form a slice retires, check every later command that still passes it. |
| aQ5 | y, developer-only | reader | plan.md Slice 3; ADR-018 | The digest guest's check differs from ADR-018 | Answered 2026-11-25: ADR-028, record 0 must equal the digest contract's canonical bytes |
| aQ6 | y | editor | design.md §2.4 | `task --version 5` applied the version 4 one-line rule | Fixed: only the framed-layout `BadInputs` applies |
| aQ7 | y | fact | plan.md Slice 4 | Legacy grants drop the spike checks of a spike-profile run | Fixed: spike-profile runs keep them until Slice 12. Checklist line proposed for fact: F10 For each "reproduce today's checks" claim, list every feature-gated check today's code runs. |
| bS6-Q1 | y, developer-only | fact | plan.md Slice 6 | `free-text-notary` cannot reach the notary's private signer | Answered 2026-11-25: ADR-028, a test-only client `free-text-ask` against a real notary |
| bK-1 | y, developer-only | editor | prd.md S-09.2 | "Nothing else" against `read_inputs` and `finish` | Answered 2026-11-25: ADR-028, S-09.2 names both |
| bK-2 | y | test | plan.md Slice 7 | `keyword.kwlist` varies by host, so SDK bytes would too | Fixed: a literal list in `generate.py` |
| cQ1 | y | test | plan.md Slices 8, 10 | "The largest accepted input" undefined and circular | Fixed: defined, with a halving procedure recorded in ADR-0139 |
| cQ2 | y | test | plan.md Slice 8 | The peak-heap method can undercount | Fixed: count in-use blocks in MicroPython's GC allocation table |
| cQ3 | y | trust | plan.md Slice 9 | Nothing produces the `golden/v5/` files | Fixed: an ignored generator test, run once, outputs committed |
| dQ12 | y | editor | plan.md Slices 1, 12 | S-01.4.e1 includes hostile cases; Slice 1 listed only 4 | Fixed: all 12 cases `both`, hostile KVM legs through `hostile.sh`; a stop if one cannot run on KVM |
| bF-01 to bF-11, aQ1 to aQ3, cF1 to cF11, d1 to d11 | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| aQ8 to aQ14, bS5-Q1 to Q5, bS6-Q2 to Q5, bS7-Q1 to Q5, cQ4 to cQ12, d13 to d23 | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (framing helper, record writer, loop bound, `ABI_2`, `live_deadline`'s home, stub answers, `bool` encoding, digest sample types, `returns`, the busy-check FIFO, who starts the notary, S-04.5 tests, the CI `needs` line); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| aK1 (editor): framed fixture against Slice 3's probe line | Fixed with aQ4 |
| aK2 (reader): digest guest's rule | Fixed with ADR-028 |
| aK3 (editor): `task --version 5` and the version 4 rule | Fixed with aQ6 |
| aK4 (fact): legacy grants against spike checks | Fixed with aQ7 |
| aK5 (reader): `DIGEST_MAX` 8,000 against `returns` 4,096 | No change in bound: today's `MAX_RESULT` already refuses it; Slice 3 stops if the largest digest reaches 4,096 |
| bK-3 (editor): S-06.1 test names in Slices 5 and 6 | Suffixed `_host` and `_attested` |
| bK-4 (test): free-text build's message before the slice | Notes changed |
| bK-5 (fact): CI's python3 | Slice 7 states the runner image supplies it, with a stop |
| cK1 (editor): S-10.1 says "task", ADR-008 "runtime" | prd changed to "runtime" |
| cK2, cK5 (editor): timing call's input and type | Uses the largest input and the triage choice type |
| cK3, cK4 (fact): golden `digest.record` anchor; two digest records | design §2.9 changed; the two records are distinct and stated |
| dK1 (editor): Slice 1's case count | Fixed with dQ12 |
| dK2 (editor): S-04.5 "keep" against "new" | New tests in Slice 12 |
| dK3 (editor): `task --version 5` removed at the pin | S-03.2.e2 re-pointed to bare `task` in Slice 11 |
| dK4 (editor): "a lower one needs version 6" | design notes ADR-027 |
| dK5 (fact): `scope.toml` has no spike row | Update dropped unless preflight finds one |
| dK6 (fact): CI `needs` and `HYALOS_OS_SPIKE` | Slice 12 removes both lines |

### Citations checked

| Citation | Holds |
|---|---|
| a `hyalos-os/audit/src/replay.rs:437-446` | partly |
| a `hyalos-os/guest/src/lib.rs:26-32` | partly |
| a `hyalos-os/monitor/src/kvm.rs:323` | partly |
| a `hyalos-os/review/README.md:33` | partly |
| b `.github/workflows/ci.yml:241-256` | partly |
| b `hyalos-os/guest/src/lib.rs:35-38` | partly |
| c `hyalos-os/attest-check/src/lib.rs:59-62` | partly |
| c `hyalos-os/monitor/src/main.rs:926` | partly |
| c `decisions/ADR-008-…:115-118` against prd S-10.1 | partly |
| c `spikes/438/sdk/draft.py:8` | partly |
| d `hyalos-os/audit/src/audit.rs:124-133` | partly |
| d `hyalos-os/review/scope.toml` spike row | no |
| d `hyalos-os/ABI.md` draft line; `one_process.sh` | no (made by Slices 2 and 10) |

The other 101 citations the readers checked held.

## Round 3 — 2026-11-27

Four new readers on new trees of `origin/main` (`bf09657`), over every slice, since round 2's fixes touched all twelve: reader a (Haiku, Slices 1, 2, 3) checked 35 citations, 32 held, 1 partly, 1 did not (`ring_tests.rs:58`, aX6) and 1 was outside the repository; reader b (Haiku, Slices 4, 5, 6) checked 45, 36 held, 7 partly and 2 named files later slices create; reader c (Haiku, Slices 7, 8, 9) checked 37, 31 held and 6 partly; reader d (Haiku, Slices 10, 11, 12) checked 24, 20 held and 4 partly. Every reader again reported the `mermaid`, layout `text` and context §4 `bash` fences on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| bQ1 | y | editor | plan.md Slice 6 | Which template a version 4 live run sends once `live.rs` passes a type | Fixed: versions 1 to 4 pass no type and keep today's template. Checklist line proposed for editor: E10 For each new argument a slice adds to a call, say what every older version passes. |
| bQ2 | y, developer-only | trust | design.md §2.7; ADR-027 | The version 5 `system` field drops today's untrusted-data warning | Answered 2026-11-27: ADR-029, the instruction then a fixed warning sentence, covered by the template digest. Checklist line proposed for trust: T8 For each guard today's request or reply carries, check the new version keeps it or an ADR drops it. |
| cQ1 | y | fact | plan.md Slice 9 | The notary reads raw 32 bytes from an owner-only `signing.key`; the plan said hex and gave no 0600 copy | Fixed: raw bytes, copied to a scratch `--dir` with mode 0600 before use. Checklist line proposed for fact: F11 For each committed key a program reads, check its format, file name and mode against the reader. |
| cQ2 | y | test | plan.md Slice 8 | No way to read the guest's memory after a process run | Fixed: a test-only helper in the monitor's tests reads the exported memory |
| cX3 | y | editor | plan.md Slice 8 | The halving procedure against a stop on any peak over 1.5 MiB | Fixed: a floor of 512 bytes; records stay 200 |
| dQ1 | y | fact | plan.md Slice 10 | `attest ask` connects only after stdin ends, so a FIFO holds no session | Fixed: the notary logs a session only when it ends, so the live busy step is dropped; concurrency 1 is taken from `Semaphore::new(1)` and the existing notary test `t_link_a_second_connection_during_a_session_is_refused_busy`, run in Slice 10's Verify (V7 covers it) |
| dQ2 | y | editor | plan.md Slice 10 | No channel for the authorisation identifier to reach the probe | Fixed: the customer step writes it to the state directory |
| dQ3 | y | fact | plan.md Slices 8, 10 | `attest ask` refuses questions over 4,096 bytes, below the 16,384 start | Fixed: the procedure starts at 4,096 |
| dQ12 | y | editor | plan.md Shared contract, Slice 12 | A protected test name lives in a file Slice 12 deletes | Fixed: Slice 12 names the tests it deletes and their replacements (E8 covers it) |
| aQ1 | n | reader | plan.md Slice 3 | Whether "reaches 4,096" means equal or above | Re-classed non-blocking: "reaches" includes equal, plan.md Slice 3 Stop conditions |
| aX1, bK5, cQ8 | n | editor | prd.md S-10.1 | The quoted ADR-008 sentence is abridged | Re-classed non-blocking: S-10.1 cites the exact lines as the verbatim source; the quote is also made whole |
| aX2 | n | reader | prd.md S-02.4 | The abi/2 digest pinned "now" | Re-classed non-blocking: S-02.4 is split across Slices 2 and 11 (plan.md Traceability, S-02.4.e1) |
| aX4, bK6 | n | reader | prd.md S-04.3.e5 | "Not a contract" against ADR-028's exact bytes | Re-classed non-blocking: a record that is not a contract is not the digest contract's bytes, so e5 holds under ADR-028 |
| aQ2, aQ3, bF1 to bF11, cQ11, cQ12, dF1 to dF11 | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| aQ4 to aQ16, bQ3 to bQ12, cQ3 to cQ13, dQ4 to dQ11 | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (test-name suffixes, reply key, raw `--answer`, stub digest, legacy type, `RELEASE` form, canary fixture, fixture paths, golden inputs, the `standin` binary, probe types, the deadline unit row, the ADR status check); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| aX3 (reader): Slice 2's Outcome against spike-run's split | Stands: Contracts and ADR-020 govern |
| aX5, bK7 (reader): `DIGEST_MAX` 8,000 against `returns` 4,096 | Stands: Slice 3 stops at 4,096 |
| aX6 (fact): `ring_tests.rs:58` test-name example | context §3 now cites `abi_tests.rs:393` |
| bK1 (reader): guest values in S-06.1 | Stands: design §2.6 states the encoding |
| bK2 (editor): "real" and "stand-in" notary in ADR-028 | Stands: an unmodified notary program, keyed with the test key |
| bK3 (editor): template for version 4 | Fixed with bQ1 |
| bK4 (editor): canary from `body()` or a fixture | Suggested path now says the fixture |
| bK8 (reader): where `ResultLimit` sits in the diagram | Diagram changed |
| cX2 (fact): key format | Fixed with cQ1 |
| cX4 (editor): CI and the generator | Outcome reworded |
| cX5 (fact): `attest standin` | Now the `standin` binary |
| cX6 (fact): two ranges for the job | One range |
| dK1 (editor): probes' guest after Slice 11 | Probes stay on triage |
| dK2, dK3 (fact, editor) | Fixed with dQ1 and dQ2 |
| dK4 (reader): S-08.3.e1 by arithmetic | Stands: the ADR's formula, with the Slice 11 tests |
| dK5 (editor): S-08.2.e2 without a step | A Verify grep added |
| dK6 (editor): protected test deleted | Fixed with dQ12 |
| dK7 (reader): ADR-023 read before ADR-024 | Stands: ADR-023's Status line names the amendment |

### Citations checked

| Citation | Holds |
|---|---|
| a `hyalos-os/monitor/src/ring_tests.rs:58` | no |
| a `wasmi` `portable-dispatch` in 2.0.3 | not checked: outside the repository |
| a `hyalos-os/guest/src/lib.rs:121-123` | partly |
| b `hyalos-os/attest/src/lib.rs:395-410`; `audit/src/contract.rs:1-63`; `guest/src/lib.rs:26-28`, `:35-38`; `monitor/src/main.rs:208-244`; `attest/src/bin/standin.rs:1-16` | partly |
| b `hyalos-os/tasks/digest/contract`; `monitor/src/contract_tests.rs` | no (made by Slice 3) |
| c `abi_tests.rs:597-643`; `live_tests.rs:452`; `kvm.rs:29-41`; `ci.yml:241-256`; ADR-008 `:115-118`; `ABI.md:70` | partly |
| d `live.rs:21`; `main.rs:784`; `attest/src/lib.rs:532`; `attest/src/main.rs:485-489` | partly |

The other 119 citations the readers checked held.

## Round 4 — 2026-11-27

The second run of the cold read. Its review panel (Fact-checker, Test engineer and Editor on Sonnet, Trust reviewer on Opus, Agent tool, Explore) read the pack first; its blocking findings were fixed, and the Editor's re-check of the fix diff found nothing blocking. Then four new readers on new trees of `origin/main` (`bf09657`), over every slice: reader a (Haiku, Slices 1, 2, 3) checked 36 citations, 32 held, 3 partly and 1 was outside the repository (the `wasmi` source); reader b (Haiku, Slices 4, 5, 6) checked 39, 36 held and 3 partly; reader c (Haiku, Slices 7, 8, 9) checked 30, 26 held and 4 partly; reader d (Haiku, Slices 10, 11, 12) checked 27, 23 held, 3 partly and 1 did not (`two_machines.sh:36-37` as the source of a `/dev/null` start, dC1). Every reader again reported the `mermaid`, layout `text` and context §4 `bash` fences on a strict reading; none found a programming language. Every tree guard was unchanged.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PT2, PT3, PT4 | y, developer-only | trust | plan.md Slice 10 | A failure before `live`, an out-of-type probe reply, and the template `attest check` enforces | Answered 2026-11-27: ADR-030 |
| PT1, PE2, PV2 | y | editor | plan.md Slice 10 | Two leftover counts of 6 tickets | Fixed: 5 everywhere |
| PE1 | y | editor | design.md §2.1; plan.md Slices 2, 3 | Two forms of `frame` | Fixed: one form, writing `<out>` |
| PE3 | y | editor | plan.md Slice 8 | S-05.2.e3 used a Slice 6 tool Slice 8 does not depend on | Fixed: a test `Source` |
| PE4 | y | editor | plan.md Slice 11 | Files left out `contract_tests.rs` | Fixed |
| PE5 | y | editor | plan.md Slice 11 | A lower ceiling and the digest guest's exact contract bytes | Fixed: the digest contract and guest are re-pinned too |
| PV1 | y | test | plan.md Slice 1 | The hostile KVM-leg count could not match | Fixed: `kvm: (result\|refused)` |
| dQ1, dC2 | y, developer-only | trust | plan.md Slice 10; ADR-010 | Who finishes the slice that holds both tools and the measurement | Answered 2026-11-27: ADR-031, the agent builds, the developer measures and accepts ADR 0140 in the PR's last commit. Checklist line proposed for trust: T9 For each slice that holds an operator step, name who finishes it and when it may merge. |
| bQ1 | y | fact | design.md §2.1, §2.2 | `accepts.records` up to 4,096 against a framing cap of 4,096 records in all | Fixed: `accepts.records` at most 4,093. Checklist line proposed for fact: F12 For each limit inside another limit, check the boundary example still reaches the inner refusal. |
| bQ2 | y | editor | design.md §2.7; plan.md Slice 6 | The printed form of a version 5 answer and how `live.rs` reads it | Fixed: a JSON value, mapped by the contract's type |
| bQ3 | y | editor | plan.md Slices 6, 9 | S-06.5.e4 needs the contract, which `attest-check` never sees | Fixed: proven by the audit in Slice 9 |
| cQ-01 | y | editor | plan.md Slice 7 | `class` and `class_` both become `class_` | Fixed: append `_` until unused |
| cQ-02 | y | fact | plan.md Slice 8 | The in-process runner drops the guest's memory | Fixed: a test-only variant returns it |
| cQ-03 | y | trust | plan.md Slice 9 | Nothing writes the committed provider root key | Fixed: the generator makes it; `standin --root-key` always (T7 covers it) |
| aQ-01 | y | editor | prd.md S-04.3.e5; plan.md Slice 3 | The host refuses a non-contract before the guest sees it | Fixed: e5's Given is a contract other than the digest contract (ADR-028) |
| dC1 | y | fact | plan.md Slice 10 | A notary started with stdin from `/dev/null` stops at once | Fixed: the probe keeps stdin open as `two_machines.sh:36-37` does |
| dC5 | y | test | plan.md Slice 10 | The secret grep matches an existing line in a changed file | Fixed: added lines only |
| re-check PE1, PE9 (round 4 diff) | y, developer-only | editor | ADR-013, ADR-022; design.md §2.2 | The 4,096 records bound against an accepted ADR, and when 0138 and 0139 are accepted | Answered 2026-11-27: ADR-032, `accepts.records` at most 4,092; each repository ADR accepted in its slice's last commit |
| re-check PE2, PE3 (round 4 diff) | y | editor | plan.md Slice 7 | A moved test still listed; the collision corpus file made by no slice | Fixed |
| PE6 to PE9, PF1 to PF6, PT5 to PT12, PV3 to PV10, re-check PE1 to PE9 | n | panel | various | Non-blocking panel findings | Adopted, except PT5 (transport key from stdout, as today, stated in the ADR's Trust line) |
| aF-01 to aF-11, bQ15 to bQ25, cQ-17, cQ-18, dF1 to dF4 | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs; the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| bK1 to bK6 (fact, editor) | Fixed with bQ1, bQ2, bQ3, bQ5, bQ8 and the S-06 rule's wording |
| cK-01 to cK-03 (editor) | Exit codes per row, e2 on the triage SDK, e1 names `read_inputs` and `finish` |
| cK-05 (trust), cK-06, cK-07 (reader) | Fixed with cQ-03; ADR-023's Status line names its amendments; the README insight is spec's |
| aX-01, aX-05 (editor) | Stale evidence path fixed; Slice 2 Outcome names the spike-run exception |
| aX-02, aX-04 (reader) | Stand: design §2.6 gives the encoding; ADR-028 amends ADR-018 |
| dC3, dC4, dC6 to dC8 (editor) | The measured run; ADR 0140 accepted in its PR (ADR-031); digest cites; returns 4,096 |

### Citations checked

| Citation | Holds |
|---|---|
| a `abi_tests.rs:597-643`; `main.rs:1002-1030`; `review/README.md:33`, `:247` | partly |
| a `wasmi` 2.0.3 `portable-dispatch` | not checked by the reader; the Fact-checker read it (`wasmi-2.0.3/src/lib.rs:70`) |
| b `replay.rs:139-156`; `attest/src/lib.rs:395-410`, `:255-294` | partly |
| c `standin.rs:84`; `chain.rs:141-155`; `draft.py:8`; `generate.py:1-80` | partly |
| d `live.rs:21-22`; `release_switches.rs:23`; `audit.rs:124-133` | partly |
| d `two_machines.sh:36-37` as a `/dev/null` start | no |

The other 114 citations the readers checked held.

## Round 5 — 2026-11-27

The second round of the second run, after the round 4 fixes and the Editor's re-check of their diff (answered by ADR-032); round 5's own fixes include ADR-033. Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 29 citations, 26 held, 2 partly and 1 not yet (`hyalos-os/tasks/`, made by Slice 3); reader b (Haiku, Slices 4, 5, 6) checked 41, 38 held, 2 partly and 1 did not (the seen record is built in `attest/src/main.rs`, not `notary.rs`); reader c (Haiku, Slices 7, 8, 9) checked 28, 25 held, 2 partly and 1 not yet (`standin --root-key`, added by Slice 9); reader d (Haiku, Slices 10, 11, 12) checked 32, 29 held, 1 partly and 2 not yet (flags and files later slices add). Every reader reported the `mermaid` and layout `text` fences, and the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| bQ1 | y | fact | plan.md Slice 6; design.md §2.7 | A version 5 answer's type and encoding in the tool's `Answer` and seen record; today's `label` is `usize` (`attest/src/lib.rs:256`) | Fixed: the version 5 closed value, its seen-record form per type, versions 1 to 4 unchanged. Checklist line proposed for fact: F13 For each value a new version widens, check the type of every field that carries it today. |
| bQ2 | y | editor | plan.md Slices 6, 9; `audit/src/answers.rs:123-126` | Which side maps a choice word to its index when the audit compares the seen answer | Fixed: one side named, `answers.rs:123-126` in Contracts and Files. Checklist line proposed for editor: E11 For each value two programs compare, say which converts it and where. |
| cQ12 | y | test | plan.md Slice 7, 7.3 | "Runs under MicroPython" with no harness | Fixed: the harness named, with its Verify line. Checklist line proposed for test: V8 For each runtime a criterion names, check a Verify step runs under that runtime. |
| cQ13 | y, developer-only | fact | plan.md Slice 8, heap measurement; ADR-011 | How to find the `heap` array and the block size in the shipped guest without a rebuild: the guest has no name section, no data export and its `micropython` module off | Answered 2026-11-27: ADR-033, an outcome search on the shipped guest with `record_bytes` = min(4,096, floor(0.75 × R_max)); no peak is read. Checklist line proposed for fact: F14 For each figure read from a built artifact, check the artifact exposes it without a rebuild. |
| dQ1 | y | test | plan.md Slice 10 | `CALL_SECONDS` needs a mean live call time no step produces | Fixed: each live run writes per-call seconds for `notary report --real-wall`. V7 covers it. |
| re-check PE2, PE3 (round 5 diff) | y, developer-only | editor | ADR-025, ADR-027; plan.md Slices 10, 11; prd.md S-08.1 | Accepted ADRs still named a live busy check and a heap check the plan no longer runs | Answered 2026-11-27: ADR-034 |
| re-check PE1, PE4 to PE7 (round 5 diff) | n | editor | design.md, plan.md Slices 6 to 10 | Leftover restatements; Slice 7's dependency on Slice 5; the search script's interface | Fixed |
| aF1 to aF11, bF1 to bF11, cQ1 to cQ11, dF1 to dF11 | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| bQ3, bQ5, bQ9, cQ14, cQ19, cQ24, dQ2, dQ3 | n | reader | plan.md Slices 6 to 10 | The seen record's home, the `body` name clash, the S-06.5.e2 notary, the raw import's return type, the last commit, `standin` flags, the timing question, the state directories | Adopted the readers' defaults |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs; the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| bC1 (reader): `attest ask`'s type argument required in design, optional in plan | Design changed: optional |
| bC2 (editor): two `attest::body` builders in one module | Today's becomes `body_v4`; the version 5 builder is `body` |
| bC3 (fact): seen record placed in the notary | Pack changed to `attest/src/main.rs:448-460` |
| bC4 (fact): `usize` label against an i32 int answer | Resolved with bQ1 |
| bC5, bC6, bC7, bC9 (reader) | Pack wording aligned; ADR-028 left as accepted |
| cC1, cC2, cC4, cC5, cC6 (reader) | Resolved with cQ12, cQ14, cQ24, cQ19; context cites `CLAUDE.md:9-12` |
| dX1 to dX8 (reader) | dX1, dX4, dX5 changed in the pack; the rest stand as worded |
| aK1, aK4 (editor) | Design §2.1 invariant moved to the run-start check; design header lists ADR-031 and ADR-032 |
| aK2, aK3 (reader) | Stand: ADR-016's Status line names its amendments; "at least" counts pass |

### Citations checked

| Citation | Holds |
|---|---|
| b "the notary's seen record" | no |
| a `abi_tests.rs:597-643`; `two_machines.sh:47`; b `live.rs:233-243`; `attest/src/lib.rs:146-165`; c `draft.py:8`; `CLAUDE.md:9-11`; d `live.rs:21` | partly |
| a `hyalos-os/tasks/`; c, d `standin --root-key`, `--delay`, `--answer`; d `CHANGES.md`, `one_process.sh` | not yet: made by a later slice |

The other citations the readers checked held.

## Round 6 — 2026-11-27

The third and last round of the second run, after the round 5 fixes (ADR-033) and the Editor's re-check of their diff (answered by ADR-034). Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 41 citations, 33 held, 7 partly and 1 was not checked (wasmi 2.0.3, outside the tree); reader b (Haiku, Slices 4, 5, 6) checked 42, 36 held, 4 partly and 2 did not (`attest/Cargo.toml:37`, ADR-011 cited for legacy grants); reader c (Haiku, Slices 7, 8, 9) checked 33, 28 held and 5 partly; reader d (Haiku, Slices 10, 11, 12) checked 41, 38 held and 3 partly. Every reader reported the `mermaid` and layout `text` fences, and the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| cQ-01, cK-03 | y, developer-only | fact | prd.md S-05.2.e2; plan.md Slice 8 | The spike's `heap` case has records of 4,098 to 4,100 bytes, over the 4,096 cap, so dropping records cannot make it fit | Answered 2026-11-27: each record is cut to `record_bytes` and committed as a new case; S-05.2.e2 reworded. F12 covers it |
| aK1 | y, developer-only | fact | prd.md S-01.4, S-01.4.e1; plan.md Slice 1 | `micropython-raw-effect` is refused and has no record | Answered 2026-11-27: every case lists `both`; each case with a result has a record; a refused case refuses the same on both backends. Checklist line proposed for fact: F15 For each "every case has X" criterion, check each listed case can have X. |
| bK1 | y | editor | design.md §2.7 | `attest::approved` refuses today's template that versions 1 to 4 still send | Fixed: today's template for versions 1 to 4, a closed shape for version 5. E10 covers it |
| dC4, dC3 | y | editor | prd.md S-08.2.e2; plan.md Slices 10, 11 | The plan read S-08.2.e2 differently from the prd, and two slices owned e1 | Fixed: the plan follows the prd's meaning; e1 split by side |
| dC2 | y | test | plan.md Slice 10; design.md §2.8 | S-08.3 checked against a deadline not live until Slice 11 | Fixed: computed in ADR 0140, not enforced, until Slice 11. V7 covers it |
| cK-06, cQ-14 | y | editor | plan.md Slice 9 | Slice 9's live version 5 runs need an entry point that only Slice 10 adds | Fixed: the in-process live path the monitor's tests use. E4 covers it |
| cK-05, cQ-15 | y | fact | plan.md Slices 6, 9; `standin.rs:35` | The default stand-in reply cannot answer a version 5 run | Fixed: version 5 runs pass `--answer`. F8 covers it |
| aF, bF, cQ-02, cQ-03, dF fences | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions and contradictions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs; the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| aK1, bK1, cK-03, cK-05, cK-06, dC2, dC3, dC4 | Blocking; resolved as in Questions |
| aK2 (reader): the prd's typed value against the guest's i32 | prd S-06.1 says the SDK gives the typed value |
| aK3, aK4, bK2, bK3, bK5, bK6, cK-01, cK-02, cK-04, cK-07, cK-08, cK-09, dC1, dC5, dC6, dC7 (reader) | Pack wording aligned |
| aK5, bK4, dC8, dC9 (reader) | Stand: resolved by the amending ADRs' Status lines |

### Citations checked

| Citation | Holds |
|---|---|
| b `attest/Cargo.toml:37`; ADR-011 for legacy grants | no |
| a `expected.tsv:5`; `main.c:142-143`; `attest/src/lib.rs:146-165`; `review/README.md:33`; `runner.rs:64-105`; `contract.rs:1-63`; b `main.rs:448-460`; `record.rs:14`; `Cargo.toml:39-48`; `notary.rs:769-776`; c ADR-008 `:115-118`; `chain.rs:141-155`; `generate.py:1-80`; `micropython-hostile-heap.json`; rcgen `certificate.rs:95-96`; d `notary.rs:1175`, `:1209`; `audit.rs:124-133`; `two_machines.sh:72` | partly |

The other citations the readers checked held.

## Round 7 — 2026-11-27

The first round of the third run. Before it, the review panel ran on the pack with round 6's fixes: Fact-checker (sonnet) 6 findings, none blocking; Test engineer (sonnet) 8, 2 blocking; Editor (sonnet) 6, 1 blocking; Trust reviewer (opus) 6, 1 blocking, developer-only (answered by ADR-035). Re-check 1 (all four roles, the Editor with the fix diff): Editor 8 (1 blocking), Test engineer 8 (1 blocking), Fact-checker 5 (none), Trust reviewer 3 (1 blocking, developer-only, held). Re-check 2: Editor 3 (none), Test engineer 6 (1 blocking, held), Fact-checker 4 (none), Trust reviewer 6 (1 blocking, developer-only, held). Re-check 1's author items were fixed before re-check 2; the three held items count as blocking here. Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 33 citations, all held; reader b (Haiku, Slices 4, 5, 6) checked 37, 32 held and 5 partly; reader c (Haiku, Slices 7, 8, 9) checked 35, 30 held, 4 partly and 1 was not checked (rcgen, outside the tree); reader d (Haiku, Slices 10, 11, 12) checked 31, 28 held, 2 partly and 1 did not (`hyalos-os/CHANGES.md`, which Slice 6 creates). Every reader reported the `mermaid` and layout `text` fences, and the `context.md` §4 commands, as code on a strict reading; reader d also found a Python program inline in a Verify command. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PT3 (re-check 1), aQ1, aQ2 | y, developer-only | trust | plan.md Shared contract, Slices 1, 2, 3, 11 | KVM steps with no owner on a host without `/dev/kvm`: Slice 1's spike-script legs, Slices 2 and 3's `HYALOS_REQUIRE_KVM` runs, Slice 11's re-run | Answered 2026-11-27 (ADR-036): CI's `hyalos-os` job proves the workspace KVM runs; every other KVM step is a developer step under ADR-035. Checklist line proposed for trust: T9 covers it |
| PT1 (re-check 2) | y, developer-only | trust | design.md §2.7; plan.md Slice 10 | `notary public` had no outcome for a `signing.key` that exists but cannot be used | Answered 2026-11-27 (ADR-036): the key reader's fixed message, nothing on standard output, no key made, exit 2, one test per refusal |
| PV1 (re-check 2) | y | test | plan.md Slice 9 | The S-10.1 sentence was checked only against a golden report the verifier wrote | Fixed: the test holds the sentence as a literal copied from Python ADR-008, and Verify greps the golden report for it |
| dQ12 | y | test | plan.md Slices 8, 11 Verify | A Python program inline in a Verify command is code | Fixed: the check is a committed test, run by name. Checklist line proposed for test: V9 For each Verify command, check it holds no program in another language, such as `python3 -c`. |
| bQ12, bC1 | y | editor | prd.md S-06.1.e1 to e3; plan.md Slices 5, 6 | The SDK's result in S-06.1's examples had no slice that builds the SDK | Fixed: Slice 7 proves the SDK side; Traceability updated. Checklist line proposed for editor: E12 For each criterion whose Then names a component, check that a slice building that component proves that clause. |
| cQ12 | y | trust | plan.md Slice 9 | No writer named for the golden stand-in log | Fixed: the forgery rig's writer, with the `golden/v5` log key. T7 covers it |
| cQ13 | y | trust | plan.md Slice 9; prd.md S-06.5.e3, e4 | No route to a validly signed presentation with a wrong request, and no check order | Fixed: `free-text-ask`'s route signed with `golden/v5/notary.key`; the audit checks the signature, then the template. T2 covers it |
| aF, bQ01-Q11, cQ1-Q11, dQ1-Q11 fences | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions; the panel's non-blocking findings | n | reader | various | Defaults the readers and roles would choose | Adopted where they set a value an executor needs (including ADR-036's shrunk bytes and ADR-010 amendment, the `provider-root.der` reuse, `records 1-5`, the skip check and the default-build CI step); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| bC1 | Blocking; resolved as in Questions |
| aK1 (reader): KVM steps listed in Verify but not as developer steps | Resolved by ADR-036 |
| aK2, aK3, aK4, bC2, bC3, bC4, bC5, bC6, bC7, bC8, cC1, cC2, cC3, cC4, cC5, dK1, dK2, dK3, dK4, dK5, dK6 (reader) | Pack wording aligned |
| cC6 (reader) | Stands: ADR-023's Status line records its amendments |

### Citations checked

| Citation | Holds |
|---|---|
| d `hyalos-os/CHANGES.md` (created by Slice 6) | no |
| b `replay.rs:315`; `main.rs:105-124`; `live.rs:233-237`; `hidden_header.py:19`, `:23`; `attest/src/lib.rs:395-410`; c `main.rs:104-124`; `common/mod.rs:236-241`; `runner.rs:64-105`; `kvm.rs`; d `two_machines.sh:72`; `attest/src/lib.rs:22` | partly |
| c rcgen `certificate.rs:95-96` | not checked |

The other citations the readers checked held.

The Editor (sonnet) re-checked the round's diff: 6 findings, 1 blocking (`provider-root.der` on a re-pin, stated two ways), all fixed; the README Brief stays the developer's wording, with the criteria and ADR-034 and ADR-036 governing concurrency. The fixes touched every slice, so round 8 read all twelve.

## Round 8 — 2026-11-27

The second round of the third run, after round 7's fixes (ADR-036) and the Editor's re-check of their diff. Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 41 citations, 38 held, 2 partly and 1 was not checked (wasmi 2.0.3, outside the tree); reader b (Haiku, Slices 4, 5, 6) checked 36, 30 held, 3 partly and 3 did not (`attest/tests/common/canary-request`, `hyalos-os/CHANGES.md` and `declared_run_as`, which later slices create); reader c (Haiku, Slices 7, 8, 9) checked 31, 27 held and 4 partly; reader d (Haiku, Slices 10, 11, 12) checked 30, 26 held, 3 partly and 1 did not (`two_machines.sh:72`). Every reader reported the `mermaid` and layout `text` fences, and the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| cQ12, cX1 | y | test | plan.md Slice 8; design.md §1 heap row | The largest input's records had no `<number>\t`, so the triage script asks none of them and the search never exercises the model | Fixed: each record is `<n>\t` and a padded title, exactly `record_bytes` long, as the spike's heap case. Checklist line proposed for test: V10 For each generated input a measurement uses, check the task under test acts on every record of it. |
| dQ1, dK4 | y | editor | plan.md Slice 10 probe steps | The ordered probe steps never started the notary, or restarted it before step 6 | Fixed: start and restart are numbered steps in the one list. E2 covers it |
| dQ2 | y | editor | plan.md Slices 6, 10 | No source for the probes' type strings, and no position for `attest ask`'s `<type>` | Fixed: the fourth positional argument; the probes hold the literals, the `choice` one checked against the committed contract. E3 covers it |
| A1 | y | author | plan.md Slice 7 Verify | The no-`python3` run expected 10 panics after round 7 raised the generator tests above 10 | Fixed: the count follows the generator test count. E2 covers it |
| aQ1 | n (re-classed) | reader | plan.md Slice 3 Files | `size-4096` and `size-4097` name no §2.2 field | Re-classed non-blocking: the Files line points at §2.2's bound edges, and §2.2 has the whole-contract 4,096-byte row (`design.md:232`); the Files line now names it |
| aF, bQ1-Q11, cQ1-Q11, dQ3-Q13 fences | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (including `Refusal::Template` and `refused: template`, `TranscriptError`, the non-UTF-8 rule, the ceiling rule, the probe's timing file); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| cX1, dK4 | Blocking; resolved as in Questions |
| aK1, aK2, aK4, aK5, bK1, bK2, bK3, bK4, bK5, bK6, bK7, cX2, cX3, cX4, cX5, cX6, cX7, dK1, dK2, dK3, dK5, dK6, dK7, dK8, dK9 (reader) | Pack wording aligned |
| aK3 (reader) | Stands: ADR-014's Status line records its amendment |

### Citations checked

| Citation | Holds |
|---|---|
| b `attest/tests/common/canary-request`, `hyalos-os/CHANGES.md`, `declared_run_as` (made by later slices); d `two_machines.sh:72` | no |
| a `contract.rs:1-63`; `attest/src/lib.rs:146-165`; b `runner.rs:64-105`; the `RELEASE` uses; `replay.rs:99-102`; c `triage.py:17-22`; `no_stand_in.rs:3`; `attest/src/lib.rs:146-165`; `main.rs:1002-1030`; d `answers.rs:204-211`; `audit.rs:123-124`; `triage.py:7`, `:16-23` | partly |
| a wasmi 2.0.3 `portable-dispatch` | not checked |

The other citations the readers checked held.

The fix for dQ16 had written a rule for choosing the ceiling C; the author removed it, since ADR-010 and ADR-031 leave C to the developer when they accept ADR 0140. The Editor (sonnet) re-checked the round's diff: 7 findings, 3 blocking. Two were author fixes (the `call time` cell defined two ways; the Verify table rows rejecting the Contracts' cell forms). The third, ADR-033 still describing title-only search records, was asked of the developer: answered 2026-11-27, ADR-037 confirms the `<n>\t` form. All were fixed before round 9.

## Round 9 — 2026-11-27

The third and last round of the third run, after round 8's fixes (ADR-037) and the Editor's re-check of their diff. Slice 12 was not read: no fix since round 8 touched it. Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 33 citations, 29 held, 3 partly and 1 did not (python `slice-03.md:126` cited as saying "uncommitted"); reader b (Haiku, Slices 4, 5, 6) checked 46, 41 held, 4 partly and 1 did not (`verify/tests/differential.rs`, which the pack does not list); reader c (Haiku, Slices 7, 8, 9) checked 36, 35 held and 1 partly; reader d (Haiku, Slices 10, 11) checked 23, 21 held, 1 partly and 1 was not checked (`attest/tests/cost.rs:118-170`). Every reader reported the `mermaid` and layout `text` fences, or the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| bQ-01, bK-1 | y | fact | plan.md Slice 4; design.md §1 Host row, §2.3 `return` row | `ResultLimit` was placed in `Host` at the return call, but the KVM holder's `Calls` has no return method and decides the return without `Host` (`kvm.rs:328-335`, `:400`), so KVM would not enforce it | Fixed: `Calls` and `Host` gain a `result(len)` check, called before any result byte is read on both backends; `kvm.rs` is in Slice 4's Files. Checklist line proposed for fact: F16 For each check the pack places in a shared component, check every backend's call path reaches it. |
| cQ-9.1 | y | trust | plan.md Slice 9; prd.md S-07.3.e2 | No method for a validly signed record holding more answers than its limit, so the audit's count check could not be reached | Fixed: made in the test-trust rig as the forgery tests make altered records, re-authorised and re-signed with the test keys, so the audit refuses on the count. T2 covers it |
| bQ-03 and the other fence rows (aF-01 to F-09, cF-1, dQ2 to Q4) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| dQ1, dK3 | n (re-classed) | reader | plan.md Slice 10 `attest check` | How `attest check` rebuilds a template that depends on the schema | Re-classed non-blocking: `approved(body, version)` reads the question, intent, position and canonical type from the request itself (`design.md:305`), and `attest-check` rebuilds exactly that version's template from them (`design.md:312`), the row Slice 10 cites |
| aQ-01, aX-01 | n (re-classed) | reader | plan.md Slice 3 | Whether new guest lines could change `digest.wasm`'s bytes | Re-classed non-blocking: Slice 3's Verify compares the rebuilt `digest.wasm` with the committed one, and its Stop condition halts the slice if they differ (`plan.md:369`, `:393`) |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (including the replay outcomes for an over-limit record, `differential.rs`, the short signing key's message and the `$remote/notary` check); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| bK-1 | Blocking; resolved as in Questions |
| aX-02, aX-03, aX-04, aX-05, bK-2, bK-3, bK-4, bK-5, cC-1, cC-2, dK1, dK2, dK4 (reader) | Pack wording aligned |
| cC-3 (reader) | Stands: ADR-033's Status line records ADR-037's amendment |

### Citations checked

| Citation | Holds |
|---|---|
| a python `slice-03.md:126` ("uncommitted"); b `verify/tests/differential.rs` (not in the pack) | no |
| a `runner.rs:64-105`; `kvm.rs:323`; `main.rs:208-244`; b `replay.rs:310-318`; `kvm.rs:328-355`; `attest/src/lib.rs:195-212`; `ci.yml:357-360`; c `main.rs:1002-1030`; d `live.rs:21-22` | partly |
| d `attest/tests/cost.rs:118-170` | not checked |

The other citations the readers checked held.

## Round 10 — 2026-11-27

The first round of the fourth run. Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 28 citations, 26 held and 2 partly; reader b (Haiku, Slices 4, 5, 6) checked 40, 37 held and 3 partly; reader c (Haiku, Slices 7, 8, 9) checked 30, 25 held, 4 partly and 1 was not opened (`cold-read.md`, as instructed); reader d (Haiku, Slices 10, 11, 12) checked 31, 29 held, 1 partly and 1 did not (`spikes/438/evidence/micropython-triage-0.json` read as needing to replay after Slice 12). Every reader reported the `mermaid` and layout `text` fences, or the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

Review panel before the readers: Fact-checker (sonnet) 3 findings, none blocking; Test engineer (sonnet) 5 findings, 1 blocking (PV1, an `#[ignore]`d criterion test passing every count); Editor (sonnet) 7 findings, 2 blocking (PE1, the Slice 3 corpus and the generator under a lower ceiling; PE2, `main.rs` missing from Slice 6's Files); Trust reviewer (opus) 7 findings, none blocking. All were fixed before the readers ran. The Editor's re-check 1 found 6 (1 blocking: Slice 10's ignored-test count ran on the wrong test binaries), fixed. Re-check 2 found 3, 1 blocking and held: Slice 11 kept `largest.record` when the ceiling drops, so its 200-question record 0 would be `BadContract` after the pin. No developer-only item arose.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PE1 (re-check 2) | y (held) | editor | plan.md Slice 11; design.md §1, §2.2 | Slice 11 kept `largest.record` with a 200-question contract when C is below 200, against ADR-027 and ADR-035 | Fixed after the round: Slice 11 re-pins every Slice 8 record, `largest.record` included; ADR 0139 is not edited and Slice 11's progress file records the new hash; Slice 8's hash check runs in Slice 8 only |
| cQ12, cX1 | y | trust | plan.md Slices 6, 9; design.md §1 `free-text-ask` row | `free-text-ask`'s one branch only skips `approved()`, so nothing could send today's four-label template (S-06.5.e3) or a version 5 template with another prompt (S-06.5.e4) | Fixed: the one branch also sends the bytes of the file `HYALOS_TEST_FREE_TEXT_BODY` names, passed through `Attest.settings`; the tests build the e2, e3 and e4 bodies themselves. Checklist line proposed for trust: T10 For each validly signed bad input the pack names, check the stand-in it names can produce exactly that input. |
| fence rows (aQ13, bF-1 to F-11, cQ01 to Q11, dQ1, dQ3, dQ4) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| dQ2, dK1 | n (re-classed) | reader | prd.md S-02.5; plan.md Slice 12 | Whether committed spike-profile records must still read after Slice 12 | Re-classed non-blocking: the spike's records "stay as history and no longer replay in a default build" (`decisions/ADR-011:19`), and a default build never knew a spike profile (`design.md:764-765`), so S-02.5's "as before" holds and no record file changes |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (among them the runner's `Request::Result` and its wire version, `attest::body` parsing the type itself, `release_digest`'s newline, the S-03.1 and S-03.4 rows, the search's command and the ticket count from `notary tickets`); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| cX1 | Blocking; resolved as in Questions |
| aC01, aC02, aC03, bK-1, bK-2, bK-3, bK-4, cX2, cX3, cX4, cX5, cX6, dK2, dK3, dK4, dK5 (reader) | Pack wording aligned, or stands as a stated deferral (aC01, aC03, dK4) |
| dK1 | Re-classed as in Questions |

### Citations checked

| Citation | Holds |
|---|---|
| d `spikes/438/evidence/micropython-triage-0.json` (read as needing to replay after Slice 12) | no; answered by ADR-011 |
| a `abi_tests.rs:12-20`; `record.rs:203-216`; b `main.rs:120`; `differential.rs:6`; `standin.rs:35`; c `audit.rs:125-132`; `live_tests.rs:836`; `guest/src/lib.rs:127-145`; d `main.rs:117-120` | partly |

The other citations the readers checked held.

## Round 11 — 2026-11-27

The second round of the fourth run, after round 10's fixes and the Editor's re-check of their diff (9 findings, none blocking, all adopted). Slice 1 was not read: no fix since round 10 touched it. Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 2, 3, 4) checked 35 citations, 32 held and 3 partly; reader b (Haiku, Slices 5, 6, 7) checked 31, 26 held, 2 partly and 3 were files the plan creates; reader c (Haiku, Slices 8, 9, 10) checked 27, 23 held, 3 partly and 1 was a file the plan creates; reader d (Haiku, Slices 11, 12) checked 28, 26 held and 2 partly. Every reader reported the `mermaid` and layout `text` fences, or the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| aQ1 | y | fact | plan.md Slice 4; design.md §2.3 `return` row | On KVM the holder accepts `Finished` checked only against `MAX_RESULT` (`runner.rs:438`), so a runner that skips `Request::Result` passes `returns.bytes` | Fixed: the `Finished` branch also calls `host.result(len)` and refuses `ResultLimit` before recording, with a test and a Stop condition. Checklist line proposed for fact: F17 For each check the pack adds before a step, check every message that can end that step passes through it. |
| dQ1 | y | editor | plan.md Slice 12 | 12.4 and 12.5 need `with_leaked_import` and an input set, both only in spike code the slice deletes (`spike_evidence_tests.rs:449`, `spike.rs:69`) | Fixed: the helper moves to `contract_evidence_tests.rs` before the spike file goes, and the tests use the triage task's committed inputs. E8 covers it (round 4) |
| cQ1, cC2 | n (re-classed) | reader | plan.md Slice 10 | How the mean live call time is produced | Re-classed non-blocking: step 8 (`plan.md:1305`) names the `HYALOS_ATTEST` wrapper writing `<out>/live-wall` and `notary report --real-wall` |
| cQ2, cC3 | n (re-classed) | reader | plan.md Slice 8 | The status while the KVM leg is the developer's | Re-classed non-blocking: Slice 8 sets the progress Status `blocked`, naming that leg, and the developer runs it (ADR-036); the Stop condition's wording was aligned |
| fence rows (aQ2 to Q12, bF1 to F3, cQ10 to Q13, dF1 to F11) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (among them `Request::Result`'s turn and order, the deleted `spike_tests.rs` block, `ABI_2`'s lint allowance, the enforced deadline row and the generator's exit codes); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| aX1, aX2, aX3, aX4, aX5, bX1, bX2, cC1, dK1, dK2, dK3, dK4, dK5 (reader) | Pack wording aligned, or stands as a stated deferral (aX1, aX2, cC1, dK1) or an amendment already recorded (dK5) |

### Citations checked

| Citation | Holds |
|---|---|
| a `record.rs:260-280`; `two_machines.sh:47`; `contract.rs:1-63`; b `main.rs:779`; `differential.rs:6`; c `audit.rs:125-132`; `main.rs:1002-1030`; `contract_tests.rs`; d `live.rs:21-22`; `audit.rs:116-132`; `main.rs:138` | partly |

The other citations the readers checked held, or name files the plan creates.

## Round 12 — 2026-11-27

The third and last round of the fourth run, after round 11's fixes and the Editor's re-check of their diff (5 findings, none blocking, all adopted). Slices 1, 9 and 10 were not read: no fix since round 11 touched them. Three readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 2, 3, 4) checked 26 citations, 22 held and 4 partly; reader b (Haiku, Slices 5, 6, 7) checked 25, 23 held and 2 partly; reader c (Haiku, Slices 8, 11, 12) checked 40, 34 held and 6 partly. Every reader reported the `mermaid` and layout `text` fences, or the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all three.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| bQ1, bK6 | y | fact | plan.md Slice 7; design.md §6 | Round 11 placed the version 5 MicroPython runs (7.3, S-09.3.e2) in `hyalos-os/monitor/tests/generate.rs`, but the monitor has no library target, so an integration test cannot reach `execute_as` (`main.rs:138`) | Fixed: those two runs move to an in-crate `generate_tests.rs`; the pure generator tests stay in `tests/generate.rs`. Checklist line proposed for fact: F18 For each test file a slice names, check the code it must call is reachable from that file. |
| cQ12, cK2 | y | fact | plan.md Slice 11 | Round 11 defined "the digest job" as CI's fuzz step, which runs `kvm::fuzz` on a `Host` and no guest (`main.rs:910-917`, `ci.yml:304`) | Fixed: Slice 11 no longer switches a digest job; nothing runs `digest.wasm` after Slice 10's probes move to triage. F1 covers it |
| cQ21, cK4 | y, developer-only | editor | plan.md Slice 12, 12.3 | 12.3 marks ADR-0136 and ADR-0137 "Accepted; superseded in part by 0138", a status the ADR README's list (`docs/adr/README.md:9`: `Proposed`, `Accepted`, `Rejected`, `Superseded by ADR-NNNN`) does not have, and no repository ADR uses | Not answered in this run: how a repository ADR records a partial supersession is the developer's rule (CLAUDE.md, Documentation rules). Held for the next run's developer batch |
| bQ2, bK2 | n (re-classed) | reader | plan.md Slice 6 | Where `attest-check` takes the version 5 answer type from | Re-classed non-blocking: `approved(body, version)` reads the canonical type from the signed request itself (`design.md:305`) |
| fence rows (aQ10, bF1 to F6, cB1 to B11) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (among them the machine-side `Channel::result`, the 18 deleted spike tests, the 8.1 test name, S-02.6.e2's single-line records and `attest::body`'s `Result`); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| bK6, cK2 | Blocking; resolved as in Questions |
| cK4 | Held, developer-only, as in Questions |
| aX1, aX2, aX3, aX4, bK1, bK3, bK4, bK5, bK7, cK1, cK3, cK5, cK6 (reader) | Pack wording aligned, or stands as a stated deferral (aX4, bK7) or the plan's stated rule (aX2, aX3, bK3) |

### Citations checked

| Citation | Holds |
|---|---|
| a `guest/src/lib.rs:121-123`; `spike.rs:84`; `contract.rs:1-63`; `runner.rs:205`; b `replay.rs:97`; `spike_tests.rs:78`; c `ci.yml:304`; `main.rs:910-917`; `record.rs:236-247`; `audit/src/lib.rs:12-13`; `docs/adr/README.md:9`; `notary.rs:1264` | partly |

The other citations the readers checked held.

## Round 13 — 2026-11-27

The first round of the fifth run. The developer answered round 12's held item before the run: ADR-0136 and ADR-0137 are marked `Superseded by ADR-0138` (pack ADR-038, amending ADR-011). Four readers, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 31 citations, 28 held and 3 partly; reader b (Haiku, Slices 4, 5, 6) checked 36, 31 held and 5 partly; reader c (Haiku, Slices 7, 8, 9) checked 33, 27 held and 6 partly; reader d (Haiku, Slices 10, 11, 12) checked 37, 31 held, 4 partly and 1 not yet (files earlier slices create). Every reader reported the `mermaid` and layout `text` fences, or the `context.md` §4 commands, as code on a strict reading; none found a programming language. The tree guards were unchanged for all four.

Panel: Fact-checker Sonnet (Agent tool, model: sonnet, Explore), 3 findings (0 blocking); Test engineer Sonnet (Agent tool, model: sonnet, Explore), 7 findings (0 blocking); Editor Sonnet (Agent tool, model: sonnet, Explore), 8 findings (0 blocking); Trust reviewer Opus (Agent tool, model: opus, Explore), 5 findings (1 blocking, developer-only). Re-check 1: Editor, Trust reviewer, Fact-checker, Test engineer, 29 findings (3 blocking, re-classed author: PT6, PV8, PV9), all fixed. Re-check 2: Editor, Trust reviewer, Test engineer, 19 findings (4 blocking, one reported by three roles: PT13 = PE18 = PV16, PV17, PV18, PV19), held, since no fix may follow the second re-check. The first panel pass was lost twice when WSL crashed: the Test engineer's `HYALOS_REQUIRE_KVM=1` run booted KVM guests under nested virtualization; memory was not the cause (at least 13.2 GB stayed free). With `/dev/kvm` made root-only by the developer, every later pass completed.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| PT1 | y, developer-only | trust | plan.md Slice 10 | T1, T2: the measurement's `live` appends under the operator ledger key in `./ledger`, forking the committed `evidence/ledger` chain and failing `s_06_1_e1` | Answered by the developer, 2026-11-27: a fresh key and chain outside the repository (pack ADR-039). Fixed before the readers |
| PT2, PT3, PT4 | n | trust | plan.md Slice 10 | T3: the 21.4 s to 30 s call-time band; an `attest check` refusal after spend; which session names the model | Adopted: a Slice 10 stop at 1.4 × the call time over 30 s; a root or notary refusal stops the path; the Model line per session and path, a difference a stop |
| PT5 | n | trust | plan.md Slice 10 | T2: the transport key comes from the notary's stdout | Stands, as today, stated in the Trust line; reported again by both re-checks |
| PT6 | y (re-classed author) | trust | plan.md Slice 10 | Re-check 1, T3: a refusal after tickets were spent in the round read two ways | Fixed: ADR-035:22 governs; "no ticket spent" is the round's spent count unchanged from `<out>/tickets-start`; attempts run out is a failed run |
| PT7 to PT12, PT14 | n | trust | plan.md Slice 10; Shared contract | Re-checks: the `attest` build, a malformed key file, the credential copies, the certificate clock, a `template` refusal, the CI run's commit, the key copies after step D | Adopted |
| PT13, PE18, PV16 | y | trust, editor, test | plan.md Slice 10 Verify | Re-check 2: Verify greps for the `grep -q -E` pipeline that step 1 now forbids | Held at re-check 2 (fix → held); fixed after the round: Verify checks step 1's `compgen -e` form |
| PV17 | y | test | plan.md Slice 10 Verify | Re-check 2: the malformed-key line runs `target/release/attest` from the root | Held; fixed after the round: run from `hyalos-os/` |
| PV18 | y | test | plan.md Slice 10, 10.6 | Re-check 2: no binary-level case shows `refused: template` | Held; fixed after the round |
| PV19 | y | test | plan.md Slice 11, S-08.3 | Re-check 2: nothing fails if `live` passes the wrong inputs to its deadline | Held; fixed after the round: a test on the path `live` uses |
| PV8 | y (re-classed author) | test | plan.md Slice 10 step 1 | Re-check 1, V2: `env \| grep -q` under pipefail misses the variable in a large environment (shown 9 of 9 runs) | Fixed: `compgen -e` into a variable; a large-environment Verify case |
| PV9 | y (re-classed author) | test | plan.md Slice 8 | Re-check 1, V6: no producer writes Slice 8's committed records | Fixed: a fourth named generator, `capsules_8_make_v5_records` |
| the other panel findings (PF1 to PF4, PE1 to PE17, PE19 to PE22, PV1 to PV7, PV10 to PV15, PV20 to PV25) | n | fact, editor, test | various | Restatements, Files lists, Verify forms | Adopted, except PF4 (a citation in round 7, which is never rewritten) |
| dQ1, dC1 | y, developer-only | reader | ADR-039; plan.md Slice 10 | ADR-039 says the fresh HOME holds nothing of the usual operator key; the probe needs `operator-transport.key` there | Answered by the developer, 2026-11-27: pack ADR-040 amends ADR-039, the HOME holds a copy of the transport key and step D deletes it. Checklist line proposed for trust: T11 For each file a step copies into a fresh home, check every accepted ADR on that home allows it |
| bQ1, bK3 | y | fact | plan.md Slice 6 | `approved` runs inside `verify()`, which the `verify` subcommand reaches with no `<type>` | Fixed: `verify` gains an optional `<type>`; a version 5 presentation without one is refused. Checklist line proposed for fact: F19 For each rule keyed on an argument, check every caller of the function that applies it has that argument |
| cQ12, cK2 | y | editor | plan.md Slice 9 | The S-10.1 and S-10.2 bullets are bare; S-10.2.e1's test and golden file are unnamed | Fixed: each example names its test and file. Checklist line proposed for editor: E13 For each Acceptance bullet, check it says how each of its examples is shown |
| cQ14 | y | test | plan.md Slice 8 | `seed-0.tsv`'s content is never given, though the golden records depend on it | Fixed: its content and source stated. Checklist line proposed for test: V11 For each committed input a record depends on, check the pack gives its bytes or source |
| dQ2, dC3 | y (re-classed from n) | test | plan.md Slice 10 Verify | `NOTARY_PUB.start` is written inside `hyalos-os/`, failing the clean-tree line on a correct build | Fixed: both files in a temporary directory outside the tree. V1 covers it |
| aQ2 | n (re-classed) | reader | plan.md Slice 3 Verify | The rebuild `cmp` compares against is unnamed | Re-classed: Slice 3 Verify names it, `(cd hyalos-os/guest && cargo build --release --locked --target wasm32-unknown-unknown)`, and the panel ran it (`same`) |
| fence rows (aQ1, aQ16, bF1 to F11, cQ1 to Q11, dF1 to F11) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking: pack-format §6 allows `bash` or `sh` for commands, `mermaid` and layout `text` |
| the readers' other questions | n | reader | various | Defaults the readers would choose | Adopted where they set a value an executor needs (among them `on_both`'s visibility, the `TranscriptError` conversion, `Host`'s `result`, the `RELEASE` shape, the search test building its own records, the version 5 `unknown profile` message, the canary-status refusal line, and Slice 12's leftover `cfg` and `PROFILE_2_BASE` range); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| bK3, cK2, dC1, dC3 | Blocking; resolved as in Questions |
| aK1 to aK5, bK1, bK2, bK4 to bK6, cK1, cK3 to cK6, dC2, dC4 to dC8 (reader) | Pack wording aligned (aK2, aK3, cK1, cK3, cK4, cK5, dC4, dC5, dC8), or stands as the plan's stated rule (aK1, bK1, bK2, bK4, bK5, bK6, dC2, dC6, dC7) or a low-severity note (aK4, aK5, cK6) |

### Citations checked

| Citation | Holds |
|---|---|
| a `abi_tests.rs:597-643`; `runner.rs:64-105`; `review/README.md:33`; b `kvm.rs:328-335`; `runner.rs:470-478`; `runner.rs:64-105`; `main.rs:425`; `ci.yml:281`; c `live_tests.rs:598`; `no_stand_in.rs:3`; `record.rs:236-247`; `guest/src/lib.rs:127-145`; `standin.rs:97`, `:218`; d `attest-check/src/lib.rs:113-115`; `live.rs:21`; `spike_tests.rs:13-52` | partly |

The other citations the readers checked held, or name files the plan creates.

## Round 14 — 2026-11-27

The Editor re-checked round 13's diff alone: 7 findings, none blocking (PE1 to PE7, leftover restatements of the `attest` build, `live_limit`, the two 6.2 tests, the spike block's 19 tests, steps C and D, S-10.2's wording and the search input), all fixed. The author also stated that a run below version 5 keeps today's limit, and that S-10.2.e1 covers the committed records of versions 1 to 3, since none of version 4 is committed. Four readers then read the slices round 13's fixes touched, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 27 citations, 24 held and 3 partly; reader b (Haiku, Slices 4, 6) checked 36, 32 held and 4 partly; reader c (Haiku, Slices 8, 9) checked 36, 32 held, 2 partly and 2 not checkable (files the slices create; the `rcgen` source outside the tree); reader d (Haiku, Slices 10, 11, 12) checked 33, 31 held and 2 partly. The fence readings recur and stay non-blocking under pack-format §6. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| aQ4, aC1 | y | reader | plan.md Slice 3 | `task` gains a second line, but `audit_cli.rs:265` and `live_tests.rs:676` compare its whole stdout with one identity, and neither file is in Slice 3 | Fixed: both compare line 1 and assert line 2 as the guest hash; both files named. Checklist line proposed for fact: F20 For each output a slice changes, find every test that reads that whole output |
| cQ1, cC5 | y | reader | plan.md Slice 9 | The golden verifier runs set no `HYALOS_TRUST_ROOT` or `HYALOS_TRUST_SERVER_NAME`, so the stand-in chain is untrusted and the runs cannot pass | Fixed: `golden/v5/provider-root.der` and `provider.test`; the address is left unset, since `attest-check` never reads it. Checklist line proposed for trust: T12 For each stand-in a verifier run trusts, name every setting the verifier reads to trust it |
| cQ2, cC2 | y | reader | plan.md Slice 8 | No step is named that writes `largest.record` | Fixed: `measure-accepts.sh` writes it from the process run and compares the KVM run's |
| cQ3, cC1 | y | reader | plan.md Slice 8 | S-05.2.e4's literal `N-1` against a one-record input, which asks nothing | Fixed: two records, literal `result ok 1 effects 1` |
| fence rows (aQ1 to aQ3, bQ1 to bQ3, cF1 to cF11, d1 to d11) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking under pack-format §6 |
| the readers' other questions | n | reader | various | Defaults | Adopted where they set a value an executor needs (among them the search's failure rule and its 16,384 bound, the triage guest path, where the probes' state directory resolves, the ceiling wording, the 30 s deadline's citation, `SpikeContract`, `From<ReadError>`, the reason count after Slice 2, `Host`'s contract field, `RELEASE`'s sample bodies); the rest stand |

### Contradictions

| Item | Resolution |
|---|---|
| aC1, cC1, cC2, cC5 | Blocking; resolved as in Questions |
| aC2 to aC6, bK1 to bK5, cC3, cC4, cC6, dK1 to dK5 | Pack wording aligned (aC2, aC5, dK3, dK4), or stands as the plan's stated rule or a low note (aC3, aC4, aC6, bK1 to bK5, cC3, cC4, cC6, dK1, dK2, dK5); cC6 is ADR-033's "a quarter" against the 4,096 cap, where the cap is the stricter bound |

### Citations checked

| Citation | Holds |
|---|---|
| a `record.rs:203-216`; `spike_tests.rs:81`; b `runner.rs:470-478`; `replay.rs:99-102`; `differential.rs:6`; `main.rs:208-244`; c `main.rs:104-124`; `guest/src/lib.rs:127-145`; d `expected.tsv` (two rows unread); `live.rs:21` | partly |

The other citations the readers checked held, or name files the plan creates.

## Round 15 — 2026-11-27

The Editor re-checked round 14's diff alone: 6 findings, 1 blocking (PE1: the `declared` run would have written `largest.record` under the search's provisional contract; fixed, the declared mode reads the committed contract), and 5 not (PE2 to PE6), all fixed. Three readers then read the slices round 14's fixes touched, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 2, 3, 6) checked 34 citations, 33 held and 1 partly; reader b (Haiku, Slices 8, 9) checked 36, 33 held, 2 partly and 1 did not (no committed version 4 record); reader c (Haiku, Slices 10, 11) checked 33, 28 held, 4 partly and 1 not yet (files earlier slices create). The fence readings recur and stay non-blocking under pack-format §6. The tree guards were unchanged for all three. This is the third round of the run, so the run ends here.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| bQ2, bK4 | y, developer-only | reader | prd.md S-02.5.e1, e3, S-10.2; plan.md Slices 3, 6, 9 | S-02.5.e1 and e3, like S-10.2.e1, name committed version 4 records; the committed records are versions 1, 1, 2 and 3 | Answered by the developer, 2026-11-27: every such row is narrowed to the committed records of versions 1 to 3, and says none of version 4 is committed; no version 4 record is added. Fixed after the run. Checklist line proposed for fact: F21 For each example that names a committed artifact, check one exists of the kind it names |
| bQ1, bK3 | y, developer-only | reader | prd.md S-03.1.e2, S-04.5.e1; plan.md Slice 8 | The PRD says a run naming `hyalos.spike/1` is refused "as an unknown profile"; a run maps `profile_for`'s error to `ReplayDiverged` (`replay.rs:429`), and Slice 8 proves only `profile_for`'s message | Answered by the developer, 2026-11-27: a run keeps `ReplayDiverged`, refused before any backend runs, with `profile_for`'s message `unknown profile hyalos.spike/1`; the PRD rows are reworded to say so. Fixed after the run |
| cQ1, cC3 | y | reader | plan.md Slice 10, 10.5 and the ceiling ADR's tables | "public log link" is never defined; `live` prints `committed: link {seq} in the public log` and the chain stays off the repository | Author's to fix in the next run: the cell holds that line's seq and the log it was committed to |
| aQ1, aX2, aX3 | y | reader | plan.md Slice 6; design.md §2.7 | Whether `approved`'s version 5 type comes from the request or from `verify`'s `<type>`, and what a mismatch prints; the plan says `ask` calls `approved`, which only `verify` does | Author's to fix in the next run |
| cC1 | y | reader | plan.md Slice 10 Verify | The malformed-key line sets no `XDG_STATE_HOME`, so the new first check refuses before the expected line | Author's to fix in the next run |
| cC2 | y | reader | plan.md Slice 11, S-08.3 | e3 is given as e2's row (L = 200, one data record); the PRD's e3 is L = 1 with 40 inputs, 1,460 s | Author's to fix in the next run |
| cC4 | n | reader | plan.md Slice 10 step 8 | The `live-wall` `ssh` omits `-o StrictHostKeyChecking=yes`, which Verify counts | Author's to fix in the next run |
| fence rows (aF1 to aF4, bQ3 to bQ5, cQ2 to cQ12) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking under pack-format §6 |
| the readers' other questions | n | reader | various | Defaults | To adopt in the next run where they set a value an executor needs (among them `spike-task`'s split, `ReadError`, the spike field and `spike.rs` in Slice 3's Files, the corpus bytes, the digest-v5 loop, the canary's request seam, the sample types' bytes, the SDK before the search, the producers of `largest.tsv` and `hostile-heap-cut.tsv`, the forced-choice input, the `MAX_QUESTION` reason, `live --version 5`'s place in argv) |

### Contradictions

| Item | Resolution |
|---|---|
| bK3, bK4, cC1 to cC4, aX2, aX3 | As in Questions |
| aX1, aX4 to aX7, bK1, bK2, bK5, bK6 | To align in the next run (aX1, aX4, bK1, bK2, bK5, bK6), or stale wording in an amended ADR (aX5, aX6), or already planned (aX7) |

### Citations checked

| Citation | Holds |
|---|---|
| a `live.rs:184`; b `record.rs:236-247`; `main.rs:1002-1030`; c `attest/src/lib.rs:552`; `audit.rs:116-132`; `attest-check/src/lib.rs:59-62`; `abi_tests.rs:51-53` | partly |
| b `hyalos-os/evidence/out/*/record.json` as version 4 records | no |

The other citations the readers checked held, or name files the plan creates.

## Round 16 — 2026-11-27

The sixth run starts here. Review panel before the readers, on a detached tree of `origin/main` (`bf09657`) with the pack copied in: Fact-checker (sonnet) 7 findings, none blocking; Test engineer (sonnet) 9 findings, 1 blocking (the corpus oracle); Editor (sonnet) 5 findings, none blocking; Trust reviewer (opus) 5 findings, all developer-only, answered by the developer in the batch as ADR-041. All were fixed. Re-check 1: Editor 8 (PE1 blocking, step D's record count), Trust reviewer 6 (PT1 blocking, the same, fixed as author with one line per path; PT2 to PT6 not blocking, developer-only, held), Fact-checker 3 (none blocking), Test engineer 4 (PV1 blocking, a positive `portable-dispatch` check); all fixed but PT2 to PT6. Re-check 2 (Editor with the diff, Test engineer, Trust reviewer): Editor 4 (none blocking: ADR-028's and ADR-041's stale wording, a `cargo metadata` row, a "stop" note), Test engineer 5 (PV1 blocking: `measure.sh --check` failed 2 of 12 runs on a correct build), Trust reviewer 5 (none blocking, developer-only). No fix follows a second re-check, so these were held, and round 16 is blocked whatever the readers found.

Four readers then read every slice, each on its own tree: reader a (Haiku, Slices 1, 2, 3) checked 33 citations, 30 held, 2 partly and 1 did not (profile 1 pins no fuel); reader b (Haiku, Slices 4, 5, 6) checked 33, 32 held and 1 partly; reader c (Haiku, Slices 7, 8, 9) checked 47, 44 held and 3 partly; reader d (Haiku, Slices 10, 11, 12) checked 40, 38 held and 2 partly. The fence readings recur and stay non-blocking under pack-format §6. All tree guards were unchanged. One empty stray file a panel role made outside its tree (`/tmp/gp.py`) was removed.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| r2PV1 | y | test | plan.md Slice 1 Verify; `spikes/438/measure.sh:52,56` | `spike-run … \| head -2` closes the pipe early; the writer panics on the broken pipe (exit 101) and `pipefail` fails the script | Fixed: Slice 1 modifies `measure.sh`, capturing the whole output before taking two lines; Verify runs `--check` ten times, expecting ten zeros. Checklist line proposed for test: V12 For each Verify pipeline into a reader that exits early, check the producer survives a closed pipe |
| aQ14, aK3 | y (raised from n) | reader | plan.md Slice 3 Files, corpus | "each `good/` file is the digest contract with one field set to its bound" cannot make the int, prompt, `asks-16`, `id-255` or `size-4096` files | Fixed: one corpus base, `hyalos-os/tasks/corpus/base`, with a choice schema, an int schema and a prompt; each twin differs from it in one path. Checklist line proposed for editor: E14 For each fixture made as "X with one field changed", check X has that field |
| cQ1 | y | reader | plan.md Slice 8 | The forced-choice records need forced answers; `Mode` has only `Record`, `Live` and `Replay`, all answered by `stub_model` | Fixed: `#[cfg(test)] Mode::Forced(Vec<i32>)` in Slice 8's Contracts and Files |
| bQ5, bX2 | y | reader | plan.md Slice 6; design.md §2.7 | Whether `attest ask` exits non-zero on an out-of-type reply; a non-zero exit with an unknown word maps to `ModelUnavailable` (`live.rs:102-113`), not `ModelRefused` | Fixed: a tool word `out-of-type`, mapped to `ModelRefused` in `live.rs` |
| bQ6, bX4 | y | reader | plan.md Slice 6 | `Refusal::Template` breaks the exhaustive match at `answers.rs:207-209` | Fixed: Slice 6 adds the arm with §2.7's text; `answers.rs` joins its Files. Checklist line proposed for fact: F22 For each new variant of an enum, find every exhaustive match on it and the slice that adds the arm |
| bQ7, bX7 | y | reader | plan.md Slice 6 | Where `attest-check` gets the version 5 type, and no test for the stop condition holding its body equal to `attest`'s | Fixed: it derives the type from the request's output schema, as `approved` does; the test `capsules_6_2_attest_check_body_equals_attest_body` is named and counted |
| bX1 | y | reader | design.md §2.7; plan.md Slice 6 | `issue_396` "calls body_v4" against a Verify that expects the version 5 body | Fixed: it calls `body` (version 5) |
| r1PT2 to r1PT6, r2PT1 to r2PT5 | n, developer-only | trust | plan.md Slice 10; ADR-039 to ADR-041 | The re-run's `HOME`; notary state after the measurement or on abandonment; the refusal before the ledger opens; the notary host's clock; the copied binaries; the send limit's outcome; the printed task identity; the transport public key's source | Answered by the developer, 2026-11-27, as ADR-042: step D also deletes the notary state and binaries, also on abandonment; the Trust line names the notary clock, the accepted `--print-task` identity and each copied binary's SHA-256 and commit; `OPERATOR_TRANSPORT_PUB` from the usual `HOME` before step C, refused if the file is missing; a fresh `HOME` per re-run; the refusal before any ledger. The send limit (r2PT1) is the author's: refused before the session, `ProviderRefused`, no ticket. Fixed |
| r2PE1, bX6 | n | editor, reader | ADR-028 | "skips `approved()`"; `attest ask` never calls `approved` | Fixed: ADR-043 (proposed) amends the wording; ADR-028's Status line only |
| r2PE2 to r2PE4, r2PV2 to r2PV5, the readers' other questions | n | various | various | Leftover restatements and defaults | Fixed where they set a value an executor needs (among them the tamper byte, the canary skip, profile 1's pins, the reasons paragraph, S-02.4, the title clip by bytes, S-05.2.e4's two records, S-10.2's versions, `true` as 1, `Request::Result`'s encode arm, Slice 10's paths and remote report, Slice 11's commits); r2PV5 left as is (the search is the dry run's) |
| fence rows | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking under pack-format §6 |

### Contradictions

| Item | Resolution |
|---|---|
| aK1, aK2, aK4, bX3, bX5, cK1 to cK4, dK-01 to dK-04 | Aligned in the pack, or an amended ADR's recorded wording (dK-01) |

### Citations checked

| Citation | Holds |
|---|---|
| a `ring_tests.rs:15` as a fuel pin | no |
| a `audit/src/lib.rs:24`, `two_machines.sh:47`; b `lib.rs:1079-1091`; c `answers.rs:35-38`, `draft.py:8`, `draft.py:51-74`; d `attest-check/src/lib.rs:59-62`, `spikes/438/guests/micropython.wasm` from `hyalos-os/` | partly |

The other citations the readers checked held, or name files the plan creates.

The Editor then re-checked this round's fix diff alone: 12 findings, 1 blocking (PE1: the Slice 3 pairing table had no row for the `keyword` files Slice 7 adds, so that test would fail after Slice 7; fixed, the table gains `keyword` and `size`, both `asks`, and a path may lie inside the table's path), and 11 not (PE2 to PE12), all fixed: among them step D's host files, the setup step that writes `.hyalos-notary/commit`, the third 6.2 test, the lower `i32` edge in the corpus, and two wording slips in ADR-042 (the probe prints the binaries it no longer copies; the 10.3 test checks the ledger after a refused run). The developer accepted ADR-043, which now corrects the wording of both ADR-028 and ADR-042. The guard was unchanged.

## Round 17 — 2026-11-27

Round 16's fixes touched every slice, so four readers read all twelve again, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 1, 2, 3) checked 33 citations, 29 held and 4 partly; reader b (Haiku, Slices 4, 5, 6) checked 41, 38 held, 1 partly, 1 did not (ADR-011 for the legacy grants) and 1 not checked; reader c (Haiku, Slices 7, 8, 9) checked 44, 38 held, 4 partly and 2 did not (`seed-0.tsv` is made by Slice 8; the files later slices create); reader d (Haiku, Slices 10, 11, 12) checked 48, 47 held and 1 partly. The fence readings recur and stay non-blocking under pack-format §6. The tree guards were unchanged for all four.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| cQ-A, cK2 | y | reader | plan.md Shared contract, Slice 8 S-03.1.e2; design.md §2.3 | Where the pre-pin version 5 gate sits against the profile check: S-03.1.e2 expects a default-build `execute_with(5, "hyalos.spike/1")` to end `ReplayDiverged`, while the Shared contract gates "a run asked for at version 5" | Fixed: the gate is at reading a version 5 record and at the two `--version 5` options only; `execute_with` has no version gate, so `profile_for` is a version 5 run's first check in every build. Checklist line proposed for editor: E15 For each gate the pack names, say where it sits against every other check a criterion's expected result depends on |
| dQ12, dK1 | n | reader | plan.md Slice 11 | The probes' `task --version 5` calls after the option is removed | Fixed: Slice 11 drops the flag from both scripts |
| bQ5 | n | reader | plan.md Slice 4 | No test for data whose total passes `accepts.bytes` | Fixed: a named Slice 4 test, counted |
| bX1, bX2, bX4, cK1, cK5, dK2, dK4, aX1, aQ9, aX3, bQ12, dQ17 | n | reader | various | Leftover wording, a missing `cmp`, a test that did not exercise RELEASE, a citation, counts, table rows, Verify order | Fixed |
| the readers' other questions | n | reader | various | Defaults | Left to the executor's stated defaults; none sets a value the pack leaves open |
| fence rows | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking under pack-format §6 |

### Contradictions

| Item | Resolution |
|---|---|
| aX2, aX4, cK6 | Wording in accepted ADRs that their Status lines record as amended (ADR-016, ADR-018), or that attributes the clip to the guest where the script clips (ADR-037); the contracts are clear, so no new ADR |
| aX5 | Context §4 describes today's `run.sh`; Slice 1 changes it as planned |
| the rest | As in Questions |

### Citations checked

| Citation | Holds |
|---|---|
| b ADR-011 for the legacy grants | no |
| a `replay.rs:436-445`, `abi_tests.rs:597-643`, `contract.rs:1-63`, `review/README.md:247`; b `live.rs:226-227`; c `draft.py:51-74`, `guest/src/lib.rs:127-145`, `chain.rs:127-138`, `attest/src/lib.rs:146`; d `live.rs:255-285` | partly |

The other citations the readers checked held, or name files the plan creates.

The Editor then re-checked this round's fix diff alone: 8 findings, none blocking (PE1 to PE8), all fixed but one aside: ADR-043's Context says `attest-check` runs `approved`, which it does not (it rebuilds the template with its own `body`); ADR-043 is accepted and its Decision is right, so the aside stays and design.md and plan.md say it correctly. The author also named the `release_with` seam in Slice 6's `RELEASE` contract. The guard was unchanged.

## Round 18 — 2026-11-27

Three readers read the slices round 17's fixes and its re-check touched, each on a detached tree of `origin/main` (`bf09657`) with the pack copied in: reader a (Haiku, Slices 3, 4) checked 42 citations, 37 held and 5 partly; reader b (Haiku, Slices 6, 8, 9) checked 39, 34 held, 4 partly and 1 did not (`guest/src/lib.rs:127-145` as a UTF-8 check); reader c (Haiku, Slices 10, 11, 12) checked 46, 40 held, 1 partly and 5 name files earlier slices create. No reader found a blocking question outside the fence readings, which stay non-blocking under pack-format §6. The tree guards were unchanged for all three.

### Questions

| Id | Blocking | Role | File | Gap | Resolution |
|---|---|---|---|---|---|
| fence rows (aQ1 to aQ11, bQ1 to bQ11, cF1 to cF11) | n | reader | design.md, plan.md, context.md fences | Fences read as code | Re-classed non-blocking under pack-format §6 |
| bK4 | n | reader | plan.md Slice 8, the digest guest bullet | Says the digest guest counts and asks a non-UTF-8 record through `guest/src/lib.rs:127-145`; the guest has no UTF-8 check, and asks any record whose number and tab parse | Left for the executor: the outcome the bullet states holds; the cited mechanism does not. Checklist line proposed for fact: F23 For each behaviour a citation is said to show, check the cited lines do that, not only that they exist |
| bQ20, bK6 | n | reader | plan.md Slice 9 S-07.5 | No assertion that the audit refuses the no-question record, which prd S-07.5.e1 requires | Default: the S-07.5.e1 test also calls `audit_record` on that record and asserts refused, exit 1 |
| cS10-1, cK1 | n | reader | plan.md Slice 10 step 8 | One `notary report --real-wall` lacks `--dir` | Default: `--dir <state>`, as the same step's later sentence and the tool's usage give |
| cS10-2, cK2 | n | reader | plan.md Slice 10, the Deadline line | L fixed at 200 where Slice 11 takes L from the contract | Default: keep L = 200 in ADR 0140; the formula is monotone in L |
| the readers' other questions and contradictions | n | reader | various | Defaults, off-by-one ranges, bare criterion bullets, amended ADRs' recorded wording | Left to the executor's stated defaults; none sets a value the pack leaves open |

### Contradictions

| Item | Resolution |
|---|---|
| aK1 to aK5, bK1 to bK7, cK1 to cK3 | Non-blocking; as in Questions, or wording in accepted ADRs their Status lines record as amended |

### Citations checked

| Citation | Holds |
|---|---|
| b `guest/src/lib.rs:127-145` as a UTF-8 check | no |
| a `kvm.rs:400`, `replay.rs:310-318`, `replay.rs:436-445`, `abi_tests.rs:164`, `two_machines.sh:47-49`; b `attest/src/main.rs:606-607`, `kvm.rs:323`, `verify/tests/cli.rs:703-722`, ADR-008 `:115-118`; c `two_machines.sh:12` | partly |

The other citations the readers checked held, or name files the plan creates.

## Result

Pass after 18 rounds (3 in this run). Round 18's readers found no blocking question; the fence readings are non-blocking under pack-format §6, and the non-blocking items carry the defaults above.

## Round 17 result

Blocked so far after 17 rounds (2 in this run). Round 17 found one blocking reader item, the author's (cQ-A, where the pre-pin gate sits), resolved in the pack, unread. Round 18, the last of this run, reads the slices the fixes touched.

## Round 16 result

Blocked so far after 16 rounds (1 in this run). Round 16 held three re-check findings and found six blocking reader items, all the author's; the developer answered the held trust questions as ADR-042. All are resolved in the pack, unread. Round 17 reads the slices the fixes touched.

## Round 15 result

Blocked after 15 rounds (3 in this run). Round 15 found six blocking items: two developer-only (bQ2, the missing version 4 records; bQ1, the reason a spike-profile run returns), answered by the developer after the run, and four the author's (cQ1, aQ1, cC1, cC2), not yet fixed. A new cold-read run is needed; the developer chose to run again.

## Round 14 result

Blocked so far after 14 rounds (2 in this run). Round 14 found four blocking reader items, all the author's, resolved in the pack, unread. Round 15, the last of this run, reads the slices the fixes touched.

## Round 13 result

Blocked so far after 13 rounds (1 in this run). Round 13 held four re-check findings and found six blocking reader items, one developer-only (dQ1, answered as ADR-040). All are resolved in the pack, unread. Round 14 reads the slices the fixes touched.

## Earlier result

Blocked after 12 rounds (3 in this run). Round 12 found three blocking items. Two were the author's, both introduced by round 11's defaults (the version 5 runs in an integration-test file, and a digest job that does not exist); both are resolved in the pack, unread. The third, cQ21, is developer-only and not answered: how ADR-0136 and ADR-0137 record a partial supersession. A new cold-read run is needed, starting with that answer.

### Checklist proposals

- aQ1, editor: E7 For each command a criterion runs before a version switch, check that the command can select that version.
- dQ3, editor: E8 For each file a slice deletes, check that every criterion it proved is proven elsewhere afterwards.
- bS6-2, fact: F8 For every literal a slice changes, find every test or file held equal to it.
- aK3, fact: F9 For every cited line that a contract calls a named constant, quote the constant's name and condition.
- cQ-02, test: V7 For each measured figure a criterion needs, name the command or reading that produces it.
- cQ-03, trust: T7 For each committed signed fixture, name who signs it and with which committed key.
- aQ4 (round 2), editor: E9 For each input form a slice retires, check every later command that still passes it.
- aQ7 (round 2), fact: F10 For each "reproduce today's checks" claim, list every feature-gated check today's code runs.
- bQ1 (round 3), editor: E10 For each new argument a slice adds to a call, say what every older version passes.
- bQ2 (round 3), trust: T8 For each guard today's request or reply carries, check the new version keeps it or an ADR drops it.
- cQ1 (round 3), fact: F11 For each committed key a program reads, check its format, file name and mode against the reader.
- dQ1 (round 4), trust: T9 For each slice that holds an operator step, name who finishes it and when it may merge.
- bQ1 (round 4), fact: F12 For each limit inside another limit, check the boundary example still reaches the inner refusal.
- bQ1 (round 5), fact: F13 For each value a new version widens, check the type of every field that carries it today.
- bQ2 (round 5), editor: E11 For each value two programs compare, say which converts it and where.
- cQ12 (round 5), test: V8 For each runtime a criterion names, check a Verify step runs under that runtime.
- cQ13 (round 5), fact: F14 For each figure read from a built artifact, check the artifact exposes it without a rebuild.
- aK1 (round 6), fact: F15 For each "every case has X" criterion, check each listed case can have X.
- dQ12 (round 7), test: V9 For each Verify command, check it holds no program in another language, such as `python3 -c`.
- bQ1 (round 12), fact: F18 For each test file a slice names, check the code it must call is reachable from that file.
- aQ1 (round 11), fact: F17 For each check the pack adds before a step, check every message that can end that step passes through it.
- cQ12 (round 10), trust: T10 For each validly signed bad input the pack names, check the stand-in it names can produce exactly that input.
- bQ-01 (round 9), fact: F16 For each check the pack places in a shared component, check every backend's call path reaches it.
- cQ12 (round 8), test: V10 For each generated input a measurement uses, check the task under test acts on every record of it.
- r2PV1 (round 16), test: V12 For each Verify pipeline into a reader that exits early, check the producer survives a closed pipe.
- aQ14 (round 16), editor: E14 For each fixture made as "X with one field changed", check X has that field.
- bQ6 (round 16), fact: F22 For each new variant of an enum, find every exhaustive match on it and the slice that adds the arm.
- cQ-A (round 17), editor: E15 For each gate the pack names, say where it sits against every other check a criterion's expected result depends on.
- bK4 (round 18), fact: F23 For each behaviour a citation is said to show, check the cited lines do that, not only that they exist.
- bQ12 (round 7), editor: E12 For each criterion whose Then names a component, check that a slice building that component proves that clause.
