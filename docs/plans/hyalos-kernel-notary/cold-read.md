# Cold read

| | |
|---|---|
| **Model** | Haiku (Agent tool, model: haiku, Explore) |
| **Input** | Pack path and repository read access only; no conversation context |
| **Rounds** | 11 |
| **Result** | Pass after 11 rounds |

## Round 1 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore). It thought through Slice 1 and Slice 2. It checked 27 citations: 25 held, 1 held partly and 1 did not. It found no programming-language code, and reported every Mermaid fence and every shell fence outside a Verify heading. Reader tree from HEAD of `worktree-plan-gated-wall`, not from `origin/main`: the spike this pack changes is not on `main` (README, Open questions).

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1 to F7 | n | `design.md` §1, §3, §4, §5; `plan.md` Dependency graph | Seven Mermaid fences, reported under the any-code rule | Re-classed non-blocking: the pack format allows a `mermaid` fence for a diagram (pack-format §6). No change |
| F8 to F10 | n | `context.md` §4 | Three shell fences under a heading not named Verify | Re-classed non-blocking: §4 is the context template's verify-commands section. The heading is now `## 4. Verify` so it reads that way |
| F11 | y | `design.md` Modifier: performance | A shell fence outside Verify | The fence is gone. The measurement command is given in the sentence |
| Q1 | y | `plan.md` Slice 1 | No candidate field named for the binding | `design.md` §2 names `metadata.user_id` and its encoding. Slice 1 Contracts lists the fallback candidates in order, and its stop condition refers to them |
| Q2 | y | `plan.md` Shared contract, Trust settings | Nothing said how the operator's side is pointed at the stand-in provider | Trust settings gained a provider address row and a paragraph: ask, verify and the notary each take the settings, and the template's host line is the approved server name |
| Q3 | y | `design.md` §2 and Modifier: integration; ADR-004 | The encrypted link was not specified | `design.md` §2 gained `The link between the two programs`: TLS 1.3, self-signed certificates pinned by public key, the first message, deadline and second-connection rule. It applies ADR-004 without changing it. Slice 2 Contracts creates it |
| Q4 | n | `plan.md` Slice 1 step 3 | The way the count gets under the notary's signature is not named | The pack stands: Slice 1's stop condition covers a library that offers no signed field |
| Q5 | n | `design.md` §2, The binding | Encoding undefined | Adopted: 64 lowercase hex characters, a dot, the position as a decimal number |
| Q6 | n | `design.md` §2 | First value not stated | Superseded by Q17: the sequence starts at 0 for each run |
| Q7 | n | `design.md` §2, Notary: one session | How the notary learns the server name | Adopted, in `The link between the two programs` |
| Q8 | n | `plan.md` Shared contract | The release identifier had no value | Adopted, in `Constants both programs share` |
| Q9 | n | `design.md` §2, Notary: start | Start configuration format | Slice 2 Contracts: command-line arguments, names recorded under Decisions |
| Q10 | n | `design.md` §2, Keys | File formats and a crash-safe save | Adopted for the save, in Slice 2 Contracts. Key file format follows the cited pattern |
| Q11 | n | `design.md` §4 | A second connection during a session | Adopted, in `The link between the two programs` |
| Q12 | n | `plan.md` Slice 2, S-01.4.e2 | Access time is unreliable as proof a file was not opened | Adopted: the dry run places a named pipe at the path |
| Q13 | n | `design.md` §2, Operator: ask | No deadline value | Adopted: 30 seconds, in the link table and the shared constants |
| Q14 | n | `prd.md` S-02.3 | Ask must compare the attestation's key with the configured one | The pack stands: `design.md` §1 and the Operator: ask contract already require it |
| Q15 | n | `plan.md` Slice 2 | The spike's old notary key and presentations | Adopted, in Slice 2 Contracts |
| Q16 | n | `plan.md` Slice 3 Verify; `context.md` §4 | No step builds the bare-metal kernel the attacks command needs | `context.md` §4 gained the two build commands, and Slice 3 Verify points at them |
| Q17 | y, developer-only | `prd.md` S-03.5; ADR-002; `plan.md` Slice 5 | A question asked again before a record's first kept answer leaves no gap in a consecutive check | Developer, 2026-11-21: the notary numbers attestations per run. ADR-006 records it and amends ADR-002. S-03.5 is reworded with examples e4 and e5. `design.md` (contracts, domain model, state machines, flows, tests, risks, threats) and Slices 1, 2 and 5 follow |
| Q18 | y | `plan.md` Slice 4 Contracts | The public log's key: its value and source | Slice 4 Contracts: pinned from Sigstore's trusted root, and its SHA-256 must equal the `logID` in every stored reply |
| Q19 | n | `plan.md` Slice 4 Verify, 4.1 | The search missed the key field the audit uses, and opening the ledger makes a key | Re-classed blocking, since the Verify would pass while the audit still opened a key. The search pattern is wider, the dry run asserts the home directory stays empty, and Contracts says opening for audit never makes a key |
| Q20 | n | `design.md` §2, Monitor: audit | The audit's inputs are not named | Slice 4 Contracts: recorded under Decisions |
| Q21 | n | `prd.md` S-04.5 | The three limits are not written | The pack stands: S-04.5 names all three |

### Contradictions

| Item | Resolution |
|---|---|
| C1: README header says 6 slices, Route says 5 | Route changed to 6, naming the cost report |
| C2: README Open questions leaves an unattested run open; prd G4 and S-02.2 refuse it | The open question now says the pack settles it |
| C3: the public log's key is an input in `design.md` and a constant in Slice 4 | Blocking. Both now say a pinned constant, not an input |
| C4: `design.md` says the link is specified in Contracts, and it was not | Blocking. Contracts now holds it (Q3) |
| C5: Slice 4 criterion 4.2 both passes and refuses with no network | Blocking. 4.2 and its dry run now separate the proof checks, which need no network, from the whole audit, which does |
| C6: ADR-002 says a discarded answer leaves a gap; the check saw only the record's own answers | Blocking. Closed by ADR-006 (Q17) |
| C7: the count is durable in `design.md` and in memory in Slice 1 | Slice 1 states the limit and that the audit does not check it until Slice 5 |
| C8: the merge-order line says Slice 1 touches the audit | The line now names the shared file |
| C9: Slice 6 edits `design.md` and did not list it | Added to Slice 6 Files |
| C10: `context.md` §1 says the audit always calls verify | It now says only when the variable is set, with the line |
| C11: Slice 4's search does not prove criterion 4.1 | Blocking. Fixed with Q19 |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155` to 166 | yes |
| `spikes/task-kernel/attest/src/main.rs:175` to 222 | yes |
| `spikes/task-kernel/attest/src/main.rs:229` | yes |
| `spikes/task-kernel/attest/src/main.rs:58`, `:339` | yes |
| `spikes/task-kernel/attest/src/main.rs:78` | yes |
| `spikes/task-kernel/attest/src/main.rs:328` | partly: the checks are at 329 to 331. Corrected to `:329` |
| `spikes/task-kernel/attest/src/main.rs:368` | yes |
| `spikes/task-kernel/attest/src/main.rs:53` | yes |
| `spikes/task-kernel/attest/src/main.rs:179`, `:202` | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:15` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:32` to 46 | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:94` to 119 | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:122` to 159 | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:55` | yes |
| `spikes/task-kernel/kernel/src/live.rs:49` to 71 | yes |
| `spikes/task-kernel/kernel/src/live.rs:59` | yes |
| `spikes/task-kernel/kernel/src/main.rs:556` | yes |
| `spikes/task-kernel/kernel/src/main.rs:595` to 606 | yes |
| `spikes/task-kernel/kernel/src/main.rs:61` to 70 | yes |
| `spikes/task-kernel/kernel/src/main.rs:612` | yes |
| `CLAUDE.md:25`, `:26` | yes |
| `CLAUDE.md:42` | yes |
| `spikes/task-kernel/ledger/00001.rekor.json` holds an inclusion proof and a signed time | yes |
| `spikes/task-kernel/hyalos/target`, named by `context.md` §4 and Slice 3 Verify | no: build output, not in the repository. Build commands added (Q16) |
| `context.md` §4: the spike has no test runner | yes |
| `spikes/task-kernel/fixtures/issues.tsv`, `guest/digest.wasm`, `out/live3.json` | yes |

## Round 2 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 3 and Slice 4. It checked 26 citations: 21 held, 4 held partly and 1 did not. It found no programming-language code, and again reported every Mermaid fence. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F-1 to F-7 | n | `plan.md` Dependency graph; `design.md` §1, §3, §4, §5 | Seven Mermaid fences, reported under the any-code rule | Re-classed non-blocking, as in round 1: the pack format allows a `mermaid` fence for a diagram (pack-format §6). No change |
| S3-1 | y | `plan.md` Slice 3 Files; Shared contract | The monitor's tests are in another crate and could not start the stand-in provider or a test notary, and nothing said how trust settings reach the tool | The stand-in provider is now a program of its own, `attest/src/bin/standin.rs`, begun in Slice 1 and completed in Slice 2. The monitor's tests start it and the notary as processes. Trust settings are environment variables the tool inherits |
| S3-2 | n | `plan.md` Slice 3 Contracts | Names of the notary settings | Adopted: environment variables recorded under Decisions, none reusing `HYALOS_NOTARY` |
| S3-3 | n | `plan.md` Slice 3 Contracts | The call deadline had no value or owner | Adopted: 30 seconds inside ask, and the monitor ends the tool at 35 |
| S3-4 | n | `plan.md` Slice 3; `design.md` §2 Monitor: live | The monitor's direct, unattested call to the provider was not said to go | Re-classed blocking: left in, a run with no notary would call the provider unattested. New local criterion 3.3 removes it, and the design's invariant says so |
| S3-5 | n | `plan.md` Slice 3 Contracts | The refusal reasons were not named | Adopted: five reasons listed in Slice 3 Contracts |
| S3-6 | n | `plan.md` Slice 3, 3.2 | No method for the round-trip time | Adopted in 3.2 |
| S4-1 | y, developer-only | `plan.md` Slice 4; `prd.md` S-04.5; `design.md` §5 and Modifier: security | The audit cannot prove the log showed every entry under the key, and the report's limits did not say so. The design's out-of-scope list and the prd's limits differed. Before Slice 5 a passing audit has not checked attestations | Developer, 2026-11-21: name it as a fourth limit. S-04.5 and its example now name four limits. `design.md` says its out-of-scope list is the threat model's and the report's wording is S-04.5's. Slice 4 Contracts says the report states that attestations were not checked, until Slice 5 |
| S4-2 | n | `plan.md` Slice 4 Contracts | Source of the log's key | The pack stands: Slice 4 Contracts already says this, as fixed in round 1 |
| S4-3 | n | `plan.md` Slice 4, S-04.2 | What anchors the inclusion proof's root | Adopted, in the new table in Slice 4 Contracts |
| S4-4 | n | `plan.md` Slice 4, S-04.2 | The bytes the signed time covers | Adopted, in the same table, with a stop condition if a stored reply fails it |
| S4-5 | n | `plan.md` Slice 4 Contracts | The notary's key was not an input of Slice 4's audit | Adopted: both keys are inputs from Slice 4 on |
| S4-6 | n | `plan.md` Slice 4 Files | No signature crate is a dependency | Adopted: `openssl` as the ledger already uses it, and the dependency file is no longer listed |
| S4-7 | n | `plan.md` Slice 4 Verify | The search will print the key field | The pack stands: Verify expects lines and asks for each to be listed |

### Contradictions

| Item | Resolution |
|---|---|
| K1: the prd's limits and the design's out-of-scope list differ | `design.md` now says which list is which (S4-1) |
| K2: Slice 4's Outcome implied stored replies are enough | The Outcome now names a connection to the public log |
| K3: the prd forbids an unattested call and the design kept today's behaviour | Blocking. Fixed with S3-4 |
| K4: no call deadline, and 30 seconds against a 10-second target | Slice 3 Contracts now gives the deadline and says the target is a different number |
| K5: `context.md` says there is no test runner and every Verify expects passing tests | `context.md` §4 and the Shared contract now say there are no tests yet and that zero tests is a failed Verify |
| K6: Slice 1 needs a local server before Slice 2 makes one | Slice 1 now starts the stand-in provider and lists its file |
| K7: the notary's key was missing from Slice 4's inputs | Fixed with S4-5 |
| K8: S-01.1 says the notary prints the public key, the design says two | The pack stands: S-01.1 is about the signing key, and the design's contract adds the transport key of ADR-004 |
| K9: ADR-002's Decision still describes the single count | The pack stands: an accepted decision is never edited. Its Status line names ADR-006 |
| K10: the graph shows no edge between Slices 1 and 4 | The pack stands: a merge order for a shared file is not a dependency |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/kernel/src/ledger.rs:35` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:94` to 119 | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:15` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:55` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:122` | partly: the function starts there, the rule is a few lines on |
| `spikes/task-kernel/kernel/src/main.rs:544` | yes |
| `spikes/task-kernel/kernel/src/main.rs:581` | yes |
| `spikes/task-kernel/kernel/src/main.rs:556`, `kernel/src/live.rs:59` | yes, and the direct call beside it was unmentioned (S3-4) |
| `spikes/task-kernel/kernel/src/main.rs:602`, `:508` | partly: 602 is the gate, 508 the call |
| `spikes/task-kernel/kernel/src/main.rs:612` | yes |
| `spikes/task-kernel/kernel/src/main.rs:36` | yes |
| `spikes/task-kernel/attest/src/main.rs:58` | yes |
| `spikes/task-kernel/attest/src/main.rs:155` to 166 | yes |
| `spikes/task-kernel/attest/src/main.rs:175` | yes |
| `spikes/task-kernel/attest/src/main.rs:229` | yes |
| `spikes/task-kernel/attest/src/main.rs:53` | yes |
| `spikes/task-kernel/attest/src/main.rs:368` | yes |
| `spikes/task-kernel/attest/src/main.rs:329` | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `spikes/task-kernel/Cargo.toml:1` | yes |
| `CLAUDE.md:42` | yes |
| `CLAUDE.md:19`, `:25`, `:26` | yes |
| `context.md` §4: no test runner | partly: the runner exists and finds no tests. Reworded (K5) |
| `spikes/task-kernel/kernel/Cargo.toml` | yes |
| `spikes/task-kernel/ledger/00000.rekor.json` | yes |
| `spikes/task-kernel/attest/tests/` | no: it does not exist until a slice makes it, and Slice 1 needed it first. Fixed (K6) |

## Round 3 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 5 and Slice 6. It checked 25 citations: 22 held, 3 held partly and none failed. It found no programming-language code, and again reported every Mermaid fence. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `design.md` §2 Notary: cost report; `plan.md` Slice 6; `prd.md` S-05.1, S-05.2 | S-05.2 has the report give the operator's wall time, and the report's contract let it read only what the notary recorded, which cannot include it | The contract now takes an optional file of the operator's measured wall times and reports them as the operator's figure, or says none was supplied. S-05.1 and S-05.2 are unchanged. Slice 6 Contracts, its dry run and its first step follow |
| M1 to M7 | n | `design.md` §1, §3, §4, §5; `plan.md` Dependency graph | Seven Mermaid fences, reported under the any-code rule | Re-classed non-blocking, as in rounds 1 and 2 (pack-format §6). No change |
| Q2 | n | `plan.md` Slice 6 step 1; `design.md` §2 The session line | Recording usage beside the session line would break its four fixed fields | Adopted: the notary reads its own usage when it reports, and peak memory is one figure for the process |
| Q3 | n | `design.md` §2 Notary: cost report | The report's command was not named | Adopted: a `report` command of the notary's program, in Slice 6 Contracts |
| Q4 | n | `plan.md` Slices 3, 4 and 5 | Whether the audit's trusted notary key is a variable or a file | Adopted: a file. Criterion 5.1 removes the variable and its gate, and Slice 3 says so |
| Q5 | n | `plan.md` Slice 5 | Where the record's intent hash comes from | Adopted, in Slice 5 Contracts, with the window's source |
| Q6 | n | `plan.md` Slice 5 Verify | The monitor cannot produce a second ask or a mismatched run | Adopted: the tests drive ask and the test notary directly, as the new paragraph in Slice 5 Verify says |
| Q7 | n | `plan.md` Slice 5 Verify | The Verify did not build the programs its tests start | Adopted, in Slices 3 and 5 |
| Q8 | n | `plan.md` Slice 5, 5.1 | Naming the test for a local criterion | The pack stands: the Shared contract's naming rule covers any ID |
| Q9 | n | `plan.md` Slice 5, S-04.4.e3 | Which notary signs before the intent is logged | Adopted, in Slice 5 Verify |
| Q10 | n | `design.md` §2 Operator: verify | Verify's output names | Slice 5 Contracts: the names Slice 1 recorded under Decisions |
| Q11 | n | `plan.md` Slice 5 Files | Which crate holds the forgery tests | Adopted: the monitor's |

### Contradictions

| Item | Resolution |
|---|---|
| X1: the release identifier comes from the build in one place and from configuration in the next sentence | The sentence now names the two trust settings it was about |
| X2: three figures in the report's contract, four in the benchmark | Blocking, with Q1. The contract now holds the fourth |
| X3: whether the report carries the operator's wall time | Blocking, with Q1 |
| X4: the audit's notary key as a variable or a file | Fixed with Q4 |
| X5: Verify does not build what the monitor's tests start | Fixed with Q7 |
| X6: ADR-002's Decision still describes the single count | The pack stands: an accepted decision is never edited. Its Status line names ADR-006 |
| X7: two keys given, three held | The pack stands: `design.md` §2 says the log's key is a pinned constant, not an input |
| X8: peak memory per attestation or per session | Fixed with Q2 |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/kernel/src/main.rs:500` | yes |
| `spikes/task-kernel/kernel/src/main.rs:602` | yes |
| `spikes/task-kernel/kernel/src/main.rs:508` | partly: 508 is the call, the gate is at 602, and both are cited |
| `spikes/task-kernel/kernel/src/main.rs:612` | yes |
| `spikes/task-kernel/kernel/src/main.rs:544`, `:574`, `:581` | yes |
| `spikes/task-kernel/kernel/src/main.rs:36` | yes |
| `spikes/task-kernel/kernel/src/main.rs:61` to 70 | yes |
| `spikes/task-kernel/kernel/src/main.rs:167`, `:171`, `:178` | partly: the three helpers were named in the wrong order. Corrected |
| `spikes/task-kernel/kernel/src/ledger.rs:15` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:25`, `:35` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:55` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:85` to 91 | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:122` to 159 | yes |
| `spikes/task-kernel/kernel/src/live.rs:59` | yes |
| `spikes/task-kernel/kernel/src/live.rs:66` | yes |
| `spikes/task-kernel/kernel/src/live.rs:75` | partly: the direct call starts there and posts at line 92 |
| `spikes/task-kernel/attest/src/main.rs:53` | yes |
| `spikes/task-kernel/attest/src/main.rs:58`, `:78`, `:339` | yes |
| `spikes/task-kernel/attest/src/main.rs:155` | yes |
| `spikes/task-kernel/attest/src/main.rs:175`, `:179`, `:202` | yes |
| `spikes/task-kernel/attest/src/main.rs:229` | yes |
| `spikes/task-kernel/attest/src/main.rs:316`, `:329`, `:368` | yes |
| `spikes/task-kernel/ledger`, four stored log replies | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `CLAUDE.md:3`, `:19`, `:25`, `:26`, `:42` | yes |

## Round 4 — 2026-11-21

A second run of the cold read, continuing the numbering. Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 5 and Slice 6, the slices round 3's fixes touched. It checked 24 citations: 22 held and 2 held partly. It found no programming-language code, and again reported every Mermaid fence. It did not raise round 3's Q1 again. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` Slice 6 Contracts, step 1 | The report is a separate command, so reading its own usage measures the wrong process, and nothing else recorded usage | `design.md` §2 gained `The usage file`, which the serving notary rewrites after every session. The report reads it. Slice 6 Contracts and step 1 follow |
| Q2 | y | `plan.md` Slice 6, S-05.2; `prd.md` S-05.2 | S-05.2 asks for wall time over the same sessions, and the slice took one call from Slice 3 | The test that drives the 40 sessions times each call and gives the report the file, so S-05.2 holds as written with no real-provider run. Slice 3's real-provider time is set beside it, named for what it is |
| Q3 | y, developer-only | `prd.md` S-04.5; `design.md` Modifier: security; `spikes/task-kernel/kernel/src/ledger.rs:151` | Running the whole task again and showing the record the operator likes is counted by the audit and neither refused nor reported as required | Developer, 2026-11-21: always state the count. S-04.5 now requires the report to say how many runs the log holds for the task, with example S-04.5.e3. The threat table, the audit contract and flow, and Slice 4's dry runs follow |
| Q4 | n | `prd.md` S-03.5 rule | The rule's heading claimed no question was asked twice, and an ask after the last kept answer passes | Adopted: the heading now says no kept answer replaced an earlier one |
| Q5 | n | `plan.md` Slice 5 Verify | The test-only way to tell the notary another run was undefined | An environment variable the operator's ask reads. A build feature was not needed: a forger can already do this, and the audit refuses it |
| Q6 | n | `plan.md` Slice 5 Contracts | Units and edges of the window | Adopted: whole seconds, both edges inside |
| Q7 | n | `plan.md` Slice 5 Contracts | Verify's output names are left to Slice 1 | The pack stands: Slice 1 records them, and Slice 5 depends on Slice 1 |
| Q8 | n | `plan.md` Slices 4 and 5 | Which verify program an auditor trusts | Adopted: one the auditor built from the pinned lockfile, given as an input |
| Q9 | n | `prd.md` S-04.3 | Records with no attestations always fail the new audit | Adopted, stated in Slice 5 Contracts as intended |
| Q10 | n | `plan.md` Slice 6 | Which sessions divide the totals | Adopted, in `The usage file` |
| Q11 | n | `design.md` §2 The session line; Modifier: performance | Which bytes are meant | The usage file holds the link's bytes, which is what the performance table reports. The session line's provider bytes stay as they are |
| Q12 | n | `prd.md` S-05.1; `design.md` | Peak memory per attestation, per session or one figure | One figure for the process, in `The usage file`. Slice 6 criterion 6.1 records it as a deviation from S-05.1's wording for the developer |
| Q13 | n | `plan.md` Slice 6 header | Slice 6 uses what Slice 2 made and depends only on Slice 3 | The pack stands: Slice 3 depends on Slice 2 |
| Q14 | n | `plan.md` Dependency graph | Slices 3 and 4 share a file with no order | Adopted: the merge-order line now covers Slices 1, 3 and 4 |
| Q15, Q16 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |

### Contradictions

| Item | Resolution |
|---|---|
| K1: S-03.5's rule heading against ADR-006's first consequence | The heading changed (Q4). ADR-006 is accepted and is not edited: its first consequence reads loosely for an ask after the last kept answer, which changes no record |
| K2: "for this run and no other" against an audit that passes a task run several times | Blocking. Closed by the developer's answer (Q3): the answer is tied to its run, and the report states how many runs there were |
| K3: the report's own usage against the serving process's | Blocking. Fixed with Q1 |
| K4: three wordings of peak memory | Fixed with Q12 |
| K5: S-05.2's same sessions against one call from Slice 3 | Blocking. Fixed with Q2 |
| K6: provider bytes against link bytes | Fixed with Q11 |
| K7: Slice 5's search expected lines after the variable is removed | The Verify now expects no output |
| K8: Slices 3 and 4 share a file with no order | Fixed with Q14 |
| K9: ADR-002's Decision still describes the single count | The pack stands, as in rounds 2 and 3 |
| K10: Slice 6's dependency omits Slice 2 | The pack stands (Q13) |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155` to 166 | yes |
| `spikes/task-kernel/attest/src/main.rs:58`, `:339` | yes |
| `spikes/task-kernel/attest/src/main.rs:78` | yes |
| `spikes/task-kernel/attest/src/main.rs:175`, `:202` | yes |
| `spikes/task-kernel/attest/src/main.rs:229` | yes |
| `spikes/task-kernel/attest/src/main.rs:53` | yes |
| `spikes/task-kernel/attest/src/main.rs:316` to 355 | yes |
| `spikes/task-kernel/attest/src/main.rs:368` | partly: the code reports the root cause with the credential replaced, as `context.md` says. Reporting by stage only is what `design.md` requires of the new program |
| `spikes/task-kernel/kernel/src/main.rs:500`, `:508` | yes |
| `spikes/task-kernel/kernel/src/main.rs:602` | yes |
| `spikes/task-kernel/kernel/src/main.rs:612` | yes |
| `spikes/task-kernel/kernel/src/main.rs:61` | yes |
| `spikes/task-kernel/kernel/src/main.rs:36` | yes |
| `spikes/task-kernel/kernel/src/main.rs:544`, `:574`, `:581` | yes |
| `spikes/task-kernel/kernel/src/live.rs:59`, `:66` | yes |
| `spikes/task-kernel/kernel/src/live.rs:75` | partly: the direct call starts there and posts at line 92 |
| `spikes/task-kernel/kernel/src/ledger.rs:15`, `:35` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:45` | yes |
| `spikes/task-kernel/kernel/src/ledger.rs:122` to 159 | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `spikes/task-kernel/ledger`, four stored log replies | yes |
| `CLAUDE.md:3`, `:42` | yes |
| `context.md` §4: no tests yet | yes |
| `spikes/task-kernel/attest/tests/`, `src/lib.rs`, `src/bin/` absent until a slice makes them | yes |

## Round 5 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 4 and Slice 6. It checked 27 citations: 25 held, 1 held partly and 1 could not be checked from the source. It found no programming-language code, and again reported every Mermaid fence. It raised none of round 4's blocking items again. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 to Q7 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |
| Q8 | y | `plan.md` Slice 4 Contracts | The public log's key itself was not in the pack, only where to get it | Slice 4 Contracts now cites the key in this repository, `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107`, with its SHA-256, which the author checked equals the stored replies' `logID`. A stop condition forbids fetching one |
| Q9 | n | `plan.md` Slice 4, Signed time | Which log index is signed, and the body's form | Adopted in the table: the top-level index and the body as stored |
| Q10 | n | `plan.md` Slice 4, Inclusion | The leaf and which index and size | Adopted in the table |
| Q11 | n | `plan.md` Slice 4 Contracts | Nothing tied a stored reply to its own link | Adopted as a new first row of the table. Re-classed blocking in effect: without it a reply for another link could stand in |
| Q12 | n | `plan.md` Slice 4 Contracts | Names and formats of the audit's inputs | The pack stands: recorded under Decisions |
| Q13 | n | `plan.md` Slice 4, S-04.5 | What "proven" lists, and Slice 4 claiming S-04.5 whole | Adopted: S-04.5 is now split, report side in Slice 4 and attestation side in Slice 5, in both Acceptance lists and the Traceability table |
| Q14 | n | `plan.md` Slice 4 Verify | The search cannot show unreachability by itself | Adopted: each line is listed with the chain of calls that reaches it |
| Q15 | n | `plan.md` Slice 4 Files | Where the audit's tests live | Adopted |
| Q16 | n | `plan.md` Slice 6; `prd.md` S-05.2 | Which wall time lets M2 be read | The report now takes the real-provider time as a second file and prints it on its own line |
| Q17 to Q19, Q23 | n | `plan.md` Slice 6 | File locations, how usage is read, the timing file's format, the report's arguments | The pack stands: the executor's choices, recorded under Decisions, as Slice 6 Contracts now says |
| Q20 | n | `prd.md` S-05.1; `design.md` | Completed or attested sessions for the threshold | Adopted: attested, for both |
| Q21 | n | `design.md` §2 The usage file | A restart resets the counters | Adopted: one life of the process, stated by the report |
| Q22 | n | `plan.md` Slice 6 Files, 6.1 | Editing `design.md` against the rule on silent edits | The Files line now says one marked entry, also written in the progress file |
| Q24 | n | `design.md` §2 Operator: ask; `plan.md` Slices 1, 3, 5 | How the intent hash and position reach ask | Adopted: arguments, stated in Slice 1 Contracts. Slice 5's variable is the test's override only |
| Q25 | n | Parent pack README | `text` fences in the parent | Outside this pack, and allowed by the format |

### Contradictions

| Item | Resolution |
|---|---|
| C1: S-05.2 lets M2 be read from the report, and Slice 6 reported only the stand-in time | Fixed with Q16 |
| C2: completed against attested sessions | Fixed with Q20 |
| C3: S-01.5.e1's `attested` against a line that also carries run and sequence | The pack stands: the outcome field is `attested` with the run and sequence, and the line still has four fields |
| C4: three ways the intent hash reaches ask | Fixed with Q24 |
| C5: where measured figures are recorded | Fixed with Q22 |
| C6: Slice 4's search against the code that reaches the key today | Fixed with Q14 |
| C7: `context.md` says every failure is one reason, the code has two | `context.md` §2 corrected |
| C8: Slice 4 claims S-04.5 while attestations wait for Slice 5 | Fixed with Q13 |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:58`, `:78`, `:155`, `:175`, `:229`, `:53`, `:339`, `:368` | yes, all eight |
| `spikes/task-kernel/kernel/src/ledger.rs:25`, `:35`, `:44` to 46, `:94`, `:85` to 91, `:151`, `:15` | yes, all seven |
| `spikes/task-kernel/kernel/src/main.rs:36`, `:500` to 522, `:602`, `:612`, `:544`, `:574`, `:581` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:66`, `:75` | yes |
| `spikes/task-kernel/kernel/src/live.rs:49` to 71 | partly: two reasons, not one. Corrected (C7) |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `spikes/task-kernel/ledger`, four stored replies | yes |
| `CLAUDE.md:42` | yes |
| `context.md` §4: `cargo test` finds none | yes |
| The known compiler warning from `kernel/src/kvm.rs` | not checkable from the source: it is compiler output |

## Round 6 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 4 and Slice 5. It checked 29 citations: 26 held and 3 held partly. It went further than reading: it verified a stored reply's signed time under the pinned key with `openssl`, and the link-hash rule on two links, and both held as Slice 4's table states them. It found no programming-language code, and again reported every Mermaid fence. It raised none of round 5's items again. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

Four of the five blocking questions have one root: the audit's first step asks the live public log, and the pack gave tests no way past it.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` Slice 4, 4.2 and Stop conditions | The stop condition on unproven log values would fire on the log's own list of entries, which 4.2 keeps | The stop condition now excepts that one answer by name and ties it to the fourth limit of S-04.5 |
| Q2 | y | `plan.md` Slice 4, S-04.1 dry run; `design.md` §6 | A stored-only test cannot complete an audit whose log step needs the network | The Shared contract's trust settings gained a `Public log` row: a stand-in log in tests, which marks the report `test trust settings`. The S-04.1 dry run uses one holding the four real replies, and names the record to use |
| Q3 | y | `plan.md` Slice 4, S-04.5.e3 | A third run of the task has no stored reply, so the dry run could not pass | The dry run now builds its own chain under test keys in a stand-in log. `design.md` §6 levels it as integration |
| Q4 | y | `plan.md` Slice 5, S-03.5.e5; Files | The test's environment variable was in no contract and its file in no list | Slice 5 Contracts now states it and Files lists the operator's program |
| Q5 | y | `plan.md` Slice 5, forgery tests; `design.md` §6 | A forged chain is refused at the log step, so the forgery tests would prove nothing about attestations | Slice 5 Contracts: every such test uses a stand-in log, and a forgery refused at the log step has not proven its row |
| Q6 | n | `plan.md` Slice 4 Contracts | Format of the two key inputs | The pack stands: recorded under Decisions |
| Q7 | n | `plan.md` Slices 3 and 4 | Whether the old variable's gate survives until Slice 5 | Adopted, in Slice 4 Contracts |
| Q8 | n | `plan.md` Slice 5 Contracts | The verify path's input, and the `attested` command | Adopted: both take the path. `design.md` §2 lists it as an audit input |
| Q9 | n | `plan.md` Slice 5 Contracts | Verify's output names wait on Slice 1 | The pack stands: Slice 5 depends on Slice 1 |
| Q10 | n | `plan.md` Slices 4 and 5 | Where presentations live | Adopted, in Slice 5 Contracts |
| Q11 | n | `plan.md` Slice 4, Inclusion | The checkpoint's signed-note form | Adopted, in Slice 4 Contracts |
| Q12 | n | `plan.md` Slice 4, S-04.5.e3 | The report's sentence against the code's | The pack stands: the example row gives the required sentence |
| Q13 | n | `plan.md` Slice 4 | No passing record was named | Adopted: `out/live2.json`, in the S-04.1 dry run |
| Q14 | n | `plan.md` Slice 5 Contracts | Units of the session time | The pack stands: whole seconds, as round 4 fixed |
| Q15 to Q21 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |

### Contradictions

| Item | Resolution |
|---|---|
| X1: a stored-only test against an audit that needs the network | Blocking. Fixed with Q2 |
| X2: three runs in a dry run, two in the stored replies | Blocking. Fixed with Q3 |
| X3: local forgery tests against the log step | Blocking. Fixed with Q5 |
| X4: the test's variable against the ask contract | Blocking. Fixed with Q4 |
| X5: two sources for the notary's key during Slice 4 | Fixed with Q7 |
| X6: the verify path as an input the design did not list | Fixed with Q8 |
| X7: peak memory per attestation in the prd, one figure in the design | The pack stands: Slice 6 criterion 6.1 records it as a deviation for the developer |
| X8: ADR-002's Decision still describes the single count | The pack stands, as in earlier rounds |
| X9: the run count is over the chain, not the log | Slice 4's S-04.5.e3 dry run now says why the chain's count is the log's |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155`, `:53`, `:58`, `:339`, `:329`, `:368`, `:175`, `:229` | yes, all |
| `spikes/task-kernel/kernel/src/ledger.rs:25`, `:35`, `:45`, `:95`, `:122`, `:15`, `:55` | yes, all |
| `spikes/task-kernel/kernel/src/ledger.rs:151` | partly: the closure is there, the count a few lines on |
| `spikes/task-kernel/kernel/src/main.rs:602`, `:612`, `:500`, `:508`, `:544`, `:574`, `:581` | yes, all |
| `spikes/task-kernel/kernel/src/main.rs:61` | partly: the field is named `attestation`, and position is the index |
| `spikes/task-kernel/kernel/src/live.rs:66` | yes |
| `spikes/task-kernel/kernel/src/live.rs:75` | partly: the call starts there and posts at line 92 |
| `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | yes: the reader recomputed the hash and it equals the log ID |
| `spikes/task-kernel/ledger`, four replies | yes |
| Slice 4's signed-time rule, on `ledger/00000.rekor.json` | yes: verified with `openssl`, and a changed time failed |
| Slice 4's link-hash rule, on two links | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `CLAUDE.md:19`, `:25`, `:26`, `:42` | yes |
| `out/live2.json`, `out/live3.json` commit to `ledger/00001.json`, `ledger/00003.json` | yes |

## Round 7 — 2026-11-21

A third run of the cold read, continuing the numbering. Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 4 and Slice 5. It checked 30 citations: 28 held and 2 held partly. It recomputed, for all four stored log replies, the link hash, the inclusion proof up to the checkpoint's root and the entry identifier, and all held. It found no programming-language code, and again reported every Mermaid fence. It raised none of round 6's five items again, and wrote: "No slice-level question blocks Slice 4 or Slice 5". Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 to Q7 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |
| Q8 | n | `plan.md` Slice 4 Contracts | The audit's command-line shape, and whether a missing key refuses | Adopted the refusal. The shape is recorded under Decisions |
| Q9 | n | `plan.md` Shared contract, Public log | How the audit is pointed at a stand-in log | The executor's choice, recorded under Decisions, as Slice 4 Contracts now says |
| Q10 | n | `plan.md` Slice 4 Contracts | A stored reply is one object keyed by the entry's identifier | Adopted |
| Q11 | n | `plan.md` Slice 4 Contracts | Where `openssl`'s temporary files go | Adopted: the system's temporary directory, never the home directory |
| Q12 | n | `plan.md` Slice 4, Signed time | Exact bytes | The pack stands: the table gives them, and round 6's reader verified the rule on a stored reply |
| Q13 | n | `plan.md` Slice 5 Contracts | Verify's output names wait on Slice 1 | The pack stands: Slice 5 depends on Slice 1 |
| Q14 | n | `plan.md` Slice 5 Contracts | The test's variable is unnamed and reachable in a release build | The pack stands: Slice 5 Contracts says why it is harmless and that its name is recorded |
| Q15 | n | `plan.md` Slice 5; `design.md` §2 | The `attested` command had no contract | `design.md` §2 gained a `Monitor: attested` row |
| Q16 | n | `plan.md` Slice 5 Contracts; `prd.md` S-03.3 | Whole seconds let a session up to a second early pass | Adopted the reader's default: built as written, with the residual and its reason stated in Slice 5 Contracts |
| Q17 | n | `plan.md` Slice 5; Shared contract | Building a chain through the ledger's writer posts to the real log | Adopted: tests write links and replies directly, stated in the Public log row |
| Q18 | n | `plan.md` Slice 5 Files | Which crate holds which tests | Adopted |
| Q19 | n | `design.md` §2 Notary: start | The server name and roots were missing from the notary's inputs | Added |
| Q20 | y, developer-only | ADR-003; `plan.md` Shared contract; `design.md` §1 | ADR-003 lets the programs share three things and nothing else, and the shared file also holds the trust settings | Re-classed blocking: an accepted decision and a contract disagreed. Developer, 2026-11-21: amend. ADR-007 records it and ADR-003's Status names it. `design.md` §1 and the Shared contract's layout row follow |

### Contradictions

| Item | Resolution |
|---|---|
| X1: the stand-in log is audit-only, and tests build chains in it | Fixed with Q17 |
| X2: what the two programs share, in `design.md`, ADR-003 and the plan | Blocking. Closed by ADR-007 (Q20) |
| X3: the notary's start inputs omit the server name and roots | Fixed with Q19 |
| X4: "falls between" against whole seconds with both edges inside | Fixed with Q16 |
| X5: a second source for the run told to the notary | The pack stands: test-only, and stated in Slice 5 Contracts |
| X6: ADR-002's Decision still describes the single count | The pack stands, as in earlier rounds |
| X7: `attested` changed with no design row | Fixed with Q15 |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/kernel/src/ledger.rs:25`, `:35`, `:45`, `:95`, `:122`, `:15`, `:71` | yes, all |
| `spikes/task-kernel/kernel/src/ledger.rs:151` | partly: the closure is there, the count a few lines on |
| `spikes/task-kernel/kernel/src/main.rs:602`, `:500`, `:508`, `:544`, `:574`, `:581`, `:556`, `:595`, `:607`, `:612`, `:36` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:49` to 73, `:75`, `:92`, `:66` | yes, all |
| `spikes/task-kernel/attest/src/main.rs:53`, `:58`, `:339`, `:155`, `:175`, `:179`, `:202`, `:229`, `:329`, `:368`, `:349` | yes, all |
| `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | yes: hash recomputed, equals the log ID |
| `spikes/task-kernel/ledger`, four stored replies | yes: link hash, inclusion proof and identifier recomputed for all four. Signatures not checked this round |
| `spikes/task-kernel/ledger/00001.json` names `00000.json` as its intent | yes |
| `spikes/task-kernel/out/live2.json` and its salt | partly: both exist, commitment not recomputed this round |
| `CLAUDE.md:3`, `:19`, `:25`, `:26`, `:42` | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `context.md` §4: `cargo test` finds none | yes |

## Round 8 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 2 and Slice 5. It checked 22 citations: 21 held and 1 held partly. It found no programming-language code, and again reported every Mermaid fence. It did not raise round 7's ADR-003 conflict again. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1 | y | `plan.md` Slice 2; `design.md` §2 The link | Nothing said how the notary gets a verified server name from the protocol library, and the spike's notary never reads one | The author read the pinned library: the proving side can reveal the server's identity, and the verifier's output then carries the verified name. `design.md` §2 now says so, citing the two lines of the spike that change. Slice 2 has a stop condition if the output carries none |
| Q2 | y | `plan.md` Slice 5 Contracts, the window; `prd.md` S-03.3.e2, S-04.4.e3 | The residual the author wrote in round 7 let a session up to a second before the intent's signed time pass, which S-03.3.e2 refuses and no limit disclosed | The author's round 7 text was wrong and is withdrawn. The edge at the intent is strict again, as the prd says, with no residual. So that honest runs are not refused, the monitor waits two seconds past the intent's signed time before the first call. The prd is unchanged, so no developer decision was needed |
| Q3 to Q5, Q8, Q9 | n | `plan.md` Slice 2 | The operator's transport key, the link's libraries, how the release identifier is built, the sequence file, the stand-in's controls | The executor's choices, recorded under Decisions, as Slice 2 Contracts now lists |
| Q6 | n | `prd.md` S-01.1.e3 | The existing key code never refuses a loosely protected file | Adopted, in Slice 2 Contracts |
| Q7 | n | `design.md` §2 The session line | The reasons for `refused` were not listed | Adopted, in `design.md` §2 |
| Q10 | n | `design.md` §2 Operator: ask | Ask prints the notary's public key today | Adopted, in Slice 2's stop condition on notary keys |
| Q11 | n | `plan.md` Slice 3 | The monitor would still name the credential file | Adopted, in Slice 3 criterion 3.3 |
| Q12, Q13 | n | `plan.md` Slice 4; `design.md` §2 | The notary key input beside the old gate; the stand-in log beside a pinned key | The pack stands: Slice 4 Contracts states both, as rounds 6 and 7 fixed |
| Q14, Q15 | n | `plan.md` Slice 5 Contracts | Names of the test's variable and the verify path argument | The pack stands: recorded under Decisions |
| Q16 | n | `plan.md` Slice 5, S-04.4.e3 | The forgery could pass in the same second | Moot: the edge is strict (Q2) |
| Q17 | n | `plan.md` Slice 1 Contracts; `prd.md` Assumptions | The slice added fallback fields the prd does not allow | Re-classed blocking in effect and fixed: Slice 1 tries `metadata.user_id` only and stops if it is refused |
| Q18 to Q24 | n | `plan.md` Dependency graph; `design.md` | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |

### Contradictions

| Item | Resolution |
|---|---|
| C1: the window's residual against S-03.3.e2 | Blocking. Fixed with Q2 |
| C2: fallback fields against the prd's stop | Fixed with Q17 |
| C3: a pinned key that tests override | The pack stands: a trust setting, marked in every report made with it |
| C4: a required key input beside an optional gate | The pack stands: Slice 4 Contracts says the gate stays until Slice 5 |
| C5: a code comment says "by stage only" and the code reports a cleaned root cause | The pack stands: `context.md` describes the code as it is, and `design.md` sets the rule for the new program |
| C6: S-01.5.e1's `attested` against an outcome that also carries run and sequence | The pack stands, as in round 5 |
| C7: ADR-002's Decision still describes the single count | The pack stands, as in earlier rounds |
| C8: the report's four limits omit the threat model's exclusions | The pack stands: `design.md` says which list is which |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155`, `:175`, `:229`, `:58`, `:78`, `:53`, `:339`, `:358`, `:202` | yes, all |
| `spikes/task-kernel/attest/src/main.rs:368` | partly: the cleaned root cause, not a stage (C5) |
| `spikes/task-kernel/kernel/src/ledger.rs:35`, `:45`, `:95`, `:25`, `:122`, `:151`, `:15`, `:71` | yes, all |
| `spikes/task-kernel/kernel/src/main.rs:602`, `:508`, `:556`, `:501`, `:544`, `:581`, `:607`, `:612`, `:36`, `:61` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:49`, `:59`, `:66`, `:75` | yes, all |
| `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | yes: hash recomputed |
| `spikes/task-kernel/ledger/00001.rekor.json` | yes |
| `CLAUDE.md:3`, `:19`, `:25`, `:26`, `:42` | yes |
| `spikes/task-kernel/attest/Cargo.toml:7`, `:10` | yes |
| `context.md` §4: no tests yet | yes |
| `spikes/task-kernel/out/live2.json` and its salt | yes |

## Round 9 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 2 and Slice 5. It checked 31 citations: 27 held and 4 held partly. It found no programming-language code, and again reported the Mermaid fences. It raised neither of round 8's items again. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

Both blocking questions asked for a fact about the protocol library that the pack assumed and did not state. The author read each in the pinned library's source.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| Q1, Q2 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |
| Q3 | y | `plan.md` Slice 2; `design.md` §2 The notary's addition | How the run and sequence get into an attestation, and under which signature | `design.md` §2 now says: one extension of the attestation, which the library's builder supports and signs with the rest. Slice 1 records its identifier and encoding under Decisions, and Slice 2 reuses them |
| Q4 | y | `plan.md` Slice 5 Contracts; `context.md` §5 | Whose clock the session time is. The window check only binds if it is the notary's | It is the notary's: the library's notary side reads its own clock when the server hello arrives, in whole seconds. `context.md` §5 and Slice 5 Contracts now say so |
| Q5 to Q8, Q11 | n | `plan.md` Slices 2 and 5 | How ask gets its settings, deferred choices, the release identifier's mechanism, argument and variable names | The executor's choices, recorded under Decisions, as the slices already say |
| Q9, Q10 | n | `plan.md` Slice 5 Contracts | Where the wait reads the intent's signed time, and how often it waits | Adopted: from the kept log reply, once before the first call |
| Q12 | n | `design.md` §2 Monitor: audit | What position means in the record | The pack stands: the binding table defines it as the zero-based index |
| Q13 | n | README Open questions | The base branch | The pack stands: the open question and the Shared contract's preflight say it |
| Q14 | n | `plan.md` Slice 2 Verify | The search implies removing the old key code | The pack stands: that is the slice |
| Q15 | n | `plan.md` Slice 5 Contracts | Whole seconds asserted, not shown | Shown now for the session time (Q4). The log's times are integers in the stored replies |
| Q16 | n | `prd.md` S-04.5 | Wording of the four limits | The pack stands: S-04.5's text |

### Contradictions

| Item | Resolution |
|---|---|
| C1: the session diagram closes an unknown peer with no line, and the contract writes one | The diagram's label now says the refused line is written |
| C2: a required key file beside the old variable in Slice 4 | Slice 4 Contracts now says nothing reads the file yet and the variable alone decides the old step until Slice 5 |
| C3: ADR-002's Decision still describes the single count | The pack stands, as in earlier rounds |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155`, `:175`, `:229`, `:58`, `:316`, `:53`, `:187`, `:368` | yes, all |
| `spikes/task-kernel/attest/src/main.rs:272` | partly: the place the proof is configured, where the reveal would be added |
| `spikes/task-kernel/attest/src/main.rs:212` | partly then, yes now: the pack did not say whose clock (Q4) |
| `spikes/task-kernel/kernel/src/ledger.rs:15`, `:25`, `:35`, `:45`, `:95`, `:94` to 119, `:71` | yes, all |
| `spikes/task-kernel/kernel/src/ledger.rs:122` to 159, `:151` | partly: the count is a few lines after the closure |
| `spikes/task-kernel/kernel/src/main.rs:36`, `:61`, `:500`, `:548`, `:556`, `:602`, `:612` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:49`, `:59`, `:66`, `:75` | yes, all |
| `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | yes: hash recomputed |
| `spikes/task-kernel/attest/Cargo.toml:10`, `spikes/task-kernel/Cargo.toml:1` | yes |
| `CLAUDE.md:19`, `:25`, `:26`, `:42` | yes |
| `spikes/task-kernel/ledger`, `out/live2.json`, `out/live3.json` | yes |
| `docs/adr/0017`, cited by the parent pack | yes |
| The parent pack's wording of ADR-0103 | partly: the rule holds, the word is the parent's |

## Round 10 — 2026-11-21

A fourth run of the cold read, continuing the numbering. Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 1 and Slice 5 and found nothing blocking in either. It checked 28 citations: 24 held and 4 held partly. It found no programming-language code, and again reported every Mermaid fence. It raised neither of round 9's items again. Its two blocking items are in Slice 3, which it was not assigned. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F-1 to F-7 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |
| Q-01 | n | `plan.md` Slice 1 | The library's extension interface cannot be checked from the repository | The pack stands: the author read it in the pinned source (round 9), and Slice 1 has the stop condition |
| Q-02 | n | `plan.md` Slice 1; Shared contract | Slice 1 needs trust settings that the library file brings in Slice 2 | The Shared contract now says Slice 1 reads its variables in the tool's one source file and Slice 2 moves them |
| Q-03, Q-05, Q-06, Q-10, Q-11, Q-12 | n | `plan.md` Slices 1, 4, 5 | Argument order, the binding's exact form, verify's output names, the audit's inputs, the hand-over of signed times, test homes | The executor's choices, recorded under Decisions. The binding's form is in `design.md` §2 |
| Q-04 | n | `plan.md` Slice 1 Files | A test directory cannot call functions of a crate with no library file | Adopted, in Slice 1 Files |
| Q-07 | n | `plan.md` Slice 1, 1.1 | Needs the operator | The pack stands: the Shared contract's Steps that need the operator |
| Q-08 | n | `plan.md` Slice 5 Contracts; `prd.md` S-03.3 | The plan refuses the intent's own second and said S-03.3.e2 requires that, which it does not say | Reworded: the strict edge is the fail-closed reading of e2 under whole seconds, and the plan says so. The reader's default is the same |
| Q-09 | n | `plan.md` Slice 5 Contracts | The test's variable is in the production tool | The pack stands, as in round 7 |
| Q-13 | n | `prd.md` S-04.5 | Whether refused runs count | The pack stands: S-04.5 counts the runs the log holds, and a refused run is one |
| Q-14 | n | `plan.md` Slice 4 Contracts | Opening the ledger makes a key | The pack stands: Slice 4 says the audit never opens or makes a private key |
| Q-15 | y | `plan.md` Slice 3, 3.3 | The monitor would still read the credential file when it starts a live run | The author's round 8 fix was incomplete. 3.3 now also removes that read and the credential the monitor holds, with their lines |
| Q-16 | y | `plan.md` Slice 3 Contracts; `design.md` §2 | Nothing said how ask tells the monitor why it refused, so the five reasons could not be told apart | `design.md` §2 gained How ask says why it refused: one word from a closed set, with the monitor's reason and the notary's session line for each. The link's first answer may now be `release`. Slice 2 and Slice 3 Contracts cite it |
| Q-17 | n | ADR-002 | Its Decision still describes the single count | The pack stands, as in earlier rounds |

### Contradictions

| Item | Resolution |
|---|---|
| C-01: 3.3 against the monitor's own read of the credential | Blocking. Fixed with Q-15 |
| C-02: "falls between" against a strict edge | Fixed with Q-08 |
| C-03: a test directory before the library file exists | Fixed with Q-04 |
| C-04: trust settings needed in Slice 1 and created in Slice 2 | Fixed with Q-02 |
| C-05: the notary's refusal words and the monitor's reasons, with no mapping | Fixed with Q-16 |
| C-06: ADR-002 against ADR-006 | The pack stands |
| C-07: `context.md` §1 cites two lines for the gate | The pack stands: the sentence cites the verify call and the gate, in that order |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155`, `:175`, `:58`, `:339`, `:53`, `:229`, `:187`, `:212`, `:329`, `:368` | yes, all |
| `spikes/task-kernel/kernel/src/main.rs:544`, `:574`, `:581`, `:602`, `:548`, `:612`, `:36`, `:61` | yes, all |
| `spikes/task-kernel/kernel/src/main.rs:508` | partly: the verify call, which is what the sentence cites it for (C-07) |
| `spikes/task-kernel/kernel/src/live.rs:49`, `:59`, `:66` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:75` | partly: the request's body starts there, the call is at 92 |
| `spikes/task-kernel/kernel/src/ledger.rs:15`, `:35`, `:45`, `:95`, `:55`, `:78` | yes, all |
| `spikes/task-kernel/kernel/src/ledger.rs:122` to 159, `:151` | partly: the count is a few lines after the closure |
| `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | yes: hash recomputed |
| `spikes/task-kernel/attest/Cargo.toml:7`, `:10` | yes |
| `spikes/task-kernel/Cargo.toml:1` | partly: the member directory is `kernel`, its package `task-kernel` |
| `CLAUDE.md:3`, `:19`, `:25`, `:26`, `:42` | yes |
| `spikes/task-kernel/ledger`, `out/live2.json`, `out/live3.json` | yes |
| No tests in the spike; the planned files absent | yes |

## Round 11 — 2026-11-21

Reader: Haiku (Agent tool, model: haiku, Explore), a new one. It thought through Slice 2 and Slice 3 and marked no question blocking except the Mermaid fences. It checked 35 citations: 30 held and 5 held partly. It found no programming-language code. It raised neither of round 10's items again, and used the new table of ask's refusal words in its restated design. Reader tree from HEAD of `worktree-plan-gated-wall`, as in round 1.

### Questions

| Id | Blocking | File | Gap | Resolution |
|---|---|---|---|---|
| F1 to F7 | n | `design.md`; `plan.md` Dependency graph | Seven Mermaid fences | Re-classed non-blocking, as in every round (pack-format §6) |
| Q8 to Q11, Q16, Q19, Q20 | n | `plan.md` Slices 2 and 3 | How ask gets its settings, the transport key, the link's libraries, start arguments, variable names, where the monitor's tests go, how the 35-second limit is held | The executor's choices, recorded under Decisions, as the slices say |
| Q12 | n | `plan.md` Slice 2 Verify | The structural search missed the function that reads the notary key | Adopted: the search also matches a call or definition of it |
| Q13 | n | `design.md` §2 Operator: ask | The output's form on success and refusal | The pack stands: the row and How ask says why it refused give both. The reader's default is the same |
| Q14 | n | `prd.md` S-01.1; `design.md` §2 Notary: start | One public key or two | The pack stands: the design's two, each labelled, satisfy the prd's one |
| Q15 | n | `plan.md` Slice 2 Contracts | Slice 1's Decisions do not exist yet | The pack stands: Slice 2 depends on Slice 1, and README rule 2 stops it otherwise |
| Q17 | n | `plan.md` Slice 3 Stop conditions | The 10-second stop had no round-trip condition | The stop is unconditional and now says so, with the round trip recorded beside it |
| Q18 | n | `plan.md` Slice 3 Verify | Which repository and issue, and the notification's own credential | The pack stands: the operator gives both and the go-ahead (Shared contract). The notification's credential is outside this pack, which concerns the model credential |

### Contradictions

| Item | Resolution |
|---|---|
| C1: which key a stand-in log carries | The Shared contract's Public log row now says: a test key, or the pinned key over stored real replies, and the report is marked either way |
| C2: a pinned key that tests override | The pack stands, as in round 8 |
| C3: the 10-second stop against a conditional target | Fixed with Q17 |
| C4: the strict edge and the wait are in Slice 5 and not in the design | `design.md` §1 and the Monitor: live row now carry both |
| C5: the structural search against the function it missed | Fixed with Q12 |
| C6: S-01.5.e1's `attested` against the fuller outcome | The pack stands, as in round 5 |
| C7: one printed key against two | The pack stands (Q14) |
| C8: ADR-002's Decision still describes the single count | The pack stands, as in earlier rounds |

### Citations checked

| Citation | Holds |
|---|---|
| `spikes/task-kernel/attest/src/main.rs:155`, `:175`, `:229`, `:58`, `:339`, `:146`, `:187`, `:272`, `:368`, `:358` | yes, all |
| `spikes/task-kernel/attest/src/main.rs:53` | partly: the sent limit, the received limit is the next line |
| `spikes/task-kernel/attest/src/main.rs:329` | partly: the shape check, the header check follows it |
| `spikes/task-kernel/attest/src/main.rs:212` | partly: the line holds, the library's side is not in the repository |
| `spikes/task-kernel/kernel/src/main.rs:36`, `:544`, `:581`, `:548`, `:556`, `:602`, `:607`, `:612`, `:61` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:18`, `:31`, `:49`, `:59` | yes, all |
| `spikes/task-kernel/kernel/src/live.rs:66`, `:75` | partly: the wait and the body's start, the reason and the call a few lines on |
| `spikes/task-kernel/kernel/src/ledger.rs:15`, `:25`, `:35`, `:71`, `:78`, `:95`, `:122`, `:151` | yes, all |
| `core/hyalos/tests/fixtures/marketplace-r10/trusted-root.json:107` | yes: hash recomputed |
| `spikes/task-kernel/ledger`, four stored replies | yes |
| `spikes/task-kernel/attest/Cargo.toml:10` | yes |
| `CLAUDE.md:19`, `:25`, `:26`, `:42` | yes |
| Slice 2's structural search, run on today's code | yes: it finds the lines the slice removes |
| `spikes/task-kernel/out/live3.json` and its presentations | yes |

## Result

Pass after 11 rounds.

Blocking items by round: 11, 4, 1, 3, 1, 5, 1, 2, 2, 2, 0. No round raised an item an earlier round had fixed. Four were the developer's, all answered on 2026-11-21: ADR-006, the fourth limit and the run count in S-04.5, and ADR-007. Round 11's non-blocking adoptions were made after its reader finished and have not been read cold.
