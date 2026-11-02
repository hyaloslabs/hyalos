# Reference job walk

Working material, not a status tracker. It traces real journeys end to end to find steps that no ADR or story covers. Accept ADRs only once every row has an owner. Issue numbers are GitHub issues in this repo.

Second re-walk: November 1, 2026, after the planning-session decisions (PR #66).

## Journey 1: "From Claude Code, I ask Hyalos to fix issue #42 in my GitHub repo and open a PR."

| Step | Owner | Remaining gap |
| --- | --- | --- |
| 1. Claude Code calls Hyalos over MCP | ADR-0010, S5.1 (#29) | **G1.** How the driving agent authenticates to the engine's MCP server, and how the operator issues that credential. S5.4 assumes "the agent's credentials" exist |
| 2. The broker fetches issue #42 and marks it tainted | ADR-0002, ADR-0005, S3.7 (#67) | **G2.** The engine-to-broker channel (launch, authentication, manifest delivery, typed values) has no story |
| 3. A reader turns the issue into typed values | ADR-0005, S4.2 (#24), S4.3 (#25) | **G3.** No story ships the concrete workflow (reader, actor, PR). E4 defines the language only |
| 4. A planner makes a plan | ADR-0016 | **D1.** A harness-run planner returns free text, but a planner must return typed values. Either define the output channel or skip the planner in v1 |
| 5. An actor writes the code in a sandbox | ADR-0016, S2.8 (#72), S3.10 (#70), S4.4 (#26) | **D2.** The actor reads repository files, which are raw text authored by contributors. ADR-0005 does not say what provenance repo contents carry, or what taint a harness accumulates from tool output inside the sandbox |
| 6. The actor calls the model | S3.2 (#17), S3.8 (#68) | None |
| 7. The model call is paid for | ADR-0002, ADR-0016, S3.8 (#68), decision #75 | Minor: `hyalos doctor` should remind the operator to set a provider-side spend cap |
| 8. The actor pushes a branch and opens a PR | S3.3 (#18), S3.4 (#19), S3.9 (#69), S4.5 (#27) | **G4.** Opening a PR is a forge API call, not a git push. No story covers it or its manifest action |
| 9. Someone approves the merge | ADR-0012, S5.4 (#32) | **D3.** Does Hyalos perform the merge in v1, or does a human merge in GitHub? The PRD's first-pass merge rate metric assumes merges. **G5.** Where the operator sets repo, protected-branch, budget and egress policy once has no story; the engine needs it to mint manifests |
| 10. Results return to Claude Code | S5.2 (#30), S5.3 (#31), S5.7 (#74) | None |

## Journey 2: "Install a third-party Agent Skill, then it auto-updates and adds a capability."

| Step | Owner | Remaining gap |
| --- | --- | --- |
| 1. The user asks the agent to install skill X | S5.5 (#33) | **G6.** Where releases are published and where install resolves them from (an index, OCI registry or GitHub releases) is not decided |
| 2. The skill is a plain public SKILL.md that was never published through a Hyalos pipeline | ADR-0006, ADR-0007 | **D4.** ADR-0006 wants to run the existing ecosystem; ADR-0007 trusts releases that went through Hyalos CI. Nobody writes a capability manifest for a skill that has none. Needs a decision, e.g. a local-install mode that starts with zero capabilities and grants by human consent |
| 3. The operator sees the capability diff and consents | S6.4 (#39), S7.5 (#46), S5.3 (#31) | None |
| 4. The release is fetched and verified | S7.1 to S7.3 (#42 to #44), S7.4 (#45) | None, for skills published through the pipeline |
| 5. The skill's instructions enter an actor's prompt | S6.5 (#40) | **D5.** ADR-0005 has no provenance class for plugin-authored text. If tainted, no skill can ever instruct an actor. If trusted, a poisoned skill steers freely within its capabilities. Suggested: plugin text is its own class, bounded by the intersection of the skill's capabilities and the job manifest |
| 6. The skill's scripts and tools run | S6.1 (#36), S6.2 (#37), S6.3 (#38) | **G7.** The capability vocabulary (the fields shared by the job manifest and plugin capabilities, and its versioning) has no story. ADR-0006 says it must stay small |
| 7. The engine notices an update | S7.5 (#46) | **G8.** Nothing polls for updates. Also unspecified: the old version keeps running while an update waits for consent |
| 8. The update adds a capability | S7.5 (#46), ADR-0012 | None |
| 9. A release is later found malicious | S7.7 (#48), S7.6 (#47) | None; revocation reaching air-gapped installs is bounded by the maximum age |

## Cross-cutting finding

ADR-0005 says layer 3 holds "by construction." That is true for what the engine passes into an actor at the start of a step. Inside a sandbox, a harness such as Claude Code reads files and tool output freely, and the type system cannot see that. For harness actors the real bounds are the job manifest, the sandbox and the broker's content gate. D2 should say so in ADR-0005 instead of leaving the claim as written.

## Result

Journey 1 has 5 gaps that need stories (G1 to G5) and 3 decisions (D1 to D3, with D2 the most consequential). Journey 2 has 3 gaps that need stories (G6 to G8) and 2 decisions (D4, D5). Until those have owners, ADRs 0002, 0005, 0006, 0007 and 0016 should stay Proposed.
