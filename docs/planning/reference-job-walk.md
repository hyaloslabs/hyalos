# Reference job walk

Working material, not a status tracker. It traces one real job end to end to find steps that no ADR or story covers. Accept ADRs only once every row has an owner. Walk a second journey afterwards: "install a third-party Agent Skill, then it auto-updates and adds a capability" (tests E6, E7 and ADR-0012).

**Reference job:** "From Claude Code, I ask Hyalos to fix issue #42 in my GitHub repo and open a PR."

| Step | Covered by | Remaining gap |
| --- | --- | --- |
| 1. Claude Code calls Hyalos over MCP | ADR-0010, S5.1 | None |
| 2. The broker fetches issue #42 and marks it tainted | ADR-0002, ADR-0005 | Forge reads are not a story |
| 3. A reader turns the issue into typed values | ADR-0005, ADR-0002 (runs in the broker), S4.2, S4.3 | None; confirm S4.3 wording |
| 4. A planner makes a plan | ADR-0005, ADR-0016 (read-only actor variant if it needs code) | None |
| 5. An actor writes the code in a sandbox | ADR-0002, ADR-0011, ADR-0016 | Spike: real harness through the forwarder (M1 gate) |
| 6. The actor calls the model | S3.2, ADR-0016 (Anthropic Messages, OpenAI Responses) | Broker protocol adapters are not a story |
| 7. The model call is paid for | ADR-0002, ADR-0016 (API keys in v1) | Subscription terms (open question 1) |
| 8. The actor pushes a branch and opens a PR | S3.3, S3.4 | GitHub App setup is not a story |
| 9. Someone approves the merge | ADR-0012, S5.4 (`hyalos approve <id>`, local key) | Update S5.4 wording |
| 10. Results return to Claude Code | S5.2, S5.3 | None |

M1 is a cut-down version of this job: a given issue text, driven from the CLI, a real harness in the sandbox, a branch pushed and a PR opened, with no secrets in the sandbox. The full job through MCP closes M2.
