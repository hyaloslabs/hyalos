# Reference job walk

Working material, not a status tracker. It traces real journeys end to end to find steps that no ADR or story covers. Accept ADRs only once every row has an owner. Issue numbers are GitHub issues in this repo.
Second re-walk: November 1, 2026, after the planning-session decisions (PR #66). Decisions D1 to D5 were made later the same day and are folded into the ADRs.

## Journey 1: "From Claude Code, I ask Hyalos to fix issue #42 in my GitHub repo and open a PR."

| Step | Owner | Remaining gap |
| --- | --- | --- |
| 1. Claude Code calls Hyalos over MCP | ADR-0010, S5.1 (#29) | **G1.** How the driving agent authenticates to the engine's MCP server, and how the operator issues that credential. S5.4 assumes "the agent's credentials" exist. Filed as #76 |
| 2. The broker fetches issue #42 and marks it tainted | ADR-0002, ADR-0005, S3.7 (#67) | **G2.** The engine-to-broker channel (launch, authentication, manifest delivery, typed values) has no story. Filed as #77 |
| 3. A reader turns the issue into typed values | ADR-0005, S4.2 (#24), S4.3 (#25) | **G3.** No story ships the concrete workflow (reader, planner, actor, PR). E4 defines the language only. Filed as #78 |
| 4. A planner makes a plan | ADR-0005, ADR-0016 (read-only actor variant; result submitted through the broker as typed values) | **D1 decided.** Filed as #84 (`hyalos-submit` and result validation) |
| 5. An actor writes the code in a sandbox | ADR-0016, S2.8 (#72), S3.10 (#70), S4.4 (#26) | **D2 decided** (ADR-0005): repo contents are forge-authored, so an actor that reads them runs tainted, and harness actors are bounded by manifest, sandbox and gate. Filed as #85 (tainted-actor manifest) |
| 6. The actor calls the model | S3.2 (#17), S3.8 (#68) | None |
| 7. The model call is paid for | ADR-0002, ADR-0016, S3.8 (#68), decision #75 | Owned by S2.3 (#11): `hyalos doctor` reminds the operator to set a provider-side spend cap |
| 8. The actor pushes a branch and opens a PR | S3.3 (#18), S3.4 (#19), S3.9 (#69), S4.5 (#27) | **G4.** Opening a PR is a forge API call, not a git push. No story covers it or its manifest action. Filed as #79 |
| 9. Someone approves the merge, and Hyalos merges | ADR-0012, S5.4 (#32) | **D3 decided** (ADR-0012): Hyalos merges after a human approval bound to the head SHA. Filed as #86 (merge through the broker). **G5.** Where the operator sets repo, protected-branch, budget and egress policy once has no story; the engine needs it to mint manifests. Filed as #80 |
| 10. Results return to Claude Code | S5.2 (#30), S5.3 (#31), S5.7 (#74) | None |

## Journey 2: "Install a third-party Agent Skill, then it auto-updates and adds a capability."

| Step | Owner | Remaining gap |
| --- | --- | --- |
| 1. The user asks the agent to install skill X | S5.5 (#33) | **G6.** Where releases are published and where install resolves them from (an index, OCI registry or GitHub releases) is not decided. Filed as #81 |
| 2. The skill is a plain public SKILL.md that was never published through a Hyalos pipeline | ADR-0006, ADR-0007 | **D4 decided** (ADR-0006): local-install mode with zero capabilities, and a human approves every grant. Filed as #87 |
| 3. The operator sees the capability diff and consents | S6.4 (#39), S7.5 (#46), S5.3 (#31) | None |
| 4. The release is fetched and verified | S7.1 to S7.3 (#42 to #44), S7.4 (#45) | None, for skills published through the pipeline |
| 5. The skill's instructions enter an actor's prompt | S6.5 (#40) | **D5 decided** (ADR-0005): plugin-authored is its own provenance class, bounded by the intersection of the plugin's capabilities and the job manifest. Filed as #88 (the class in the type system) |
| 6. The skill's scripts and tools run | S6.1 (#36), S6.2 (#37), S6.3 (#38) | **G7.** The capability vocabulary (the fields shared by the job manifest and plugin capabilities, and its versioning) has no story. ADR-0006 says it must stay small. Filed as #82 |
| 7. The engine notices an update | S7.5 (#46) | **G8.** Nothing polls for updates. Also unspecified: the old version keeps running while an update waits for consent. Filed as #83 |
| 8. The update adds a capability | S7.5 (#46), ADR-0012 | None |
| 9. A release is later found malicious | S7.7 (#48), S7.6 (#47) | None; revocation reaching air-gapped installs is bounded by the maximum age |

## Cross-cutting finding, now in ADR-0005

"By construction" covers the values the engine passes into a step. Inside a sandbox the type system cannot see what a harness reads. ADR-0005 now says so, and treats any actor that reads forge-authored content as tainted.

## Result

All five decisions (D1 to D5) are made and recorded in ADR-0005, 0006, 0007, 0012 and 0016. Every gap and decision has an owning issue: the eight gap stories are #76 to #83, and the five stories that follow from the decisions are #84 to #88 (S4.7, S3.14, S5.9, S6.7, S4.8). With every row owned, the ADRs can be accepted once the remaining open questions are answered.
