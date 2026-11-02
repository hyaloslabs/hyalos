# PRD: Hyalos

**Status:** Draft for the planning session.
**Scope of this doc:** intent and scope only, never status. Status lives in GitHub issues ([ADR-0014](../adr/0014-tracker-holds-status.md)).

## Problem

Agents are being given real tools on real machines, and the failures are now public: a poisoned extension served from Cursor's marketplace for about 11 hours reached up to 5,000 developer machines. Hyperscalers are answering at the isolation layer (Google's Agent Compute Engine gives each agent a microVM with credentials outside it), but isolation alone does not decide what an agent may do, who wrote the code it runs, or what a steered agent may write.

Today's hivrr solves most of that, but as about 15 services, a long trail of retrofits and an install that is a barrier. It is a record of lessons more than a product to introduce.

## Vision

**One tool an agent can use without getting you into trouble.** Any agent (Claude Code, Hermes, an Archon workflow) hands work to Hyalos; Hyalos runs it contained, within a signed manifest, with every action audited, and runs any Agent Skill or MCP server safely.

## Goals

1. A single install that reaches a first contained job in under 10 minutes.
2. A trusted core of at most 30,000 lines that one person can review end to end.
3. Driven entirely by an agent over MCP; humans set policy and approve only irreversible actions.
4. Run untrusted Agent Skills, MCP servers and plugins with enforced capabilities, not publisher trust.
5. Security properties enforced by construction (types, manifests, policy analysis), not by convention.

## Non-goals for v1

- A web dashboard (approvals and results are shown in the driving client, as MCP Apps where supported).
- Trackers beyond GitHub, GitLab and a local tracker; notification channels beyond one webhook.
- Autonomy modes, complex budgeting, live-config tiers.
- Hosting a public service. v1 is self-hosted, MIT-licensed open source.
- Migrating data or code from today's hivrr.
- macOS and native Windows hosts, and signing in with a chat subscription instead of an API key. Both are deferred, with no schedule ([ADR-0011](../adr/0011-pluggable-substrates.md), [ADR-0016](../adr/0016-agent-runtime-and-broker-protocols.md)).

## Users

- **The driving agent:** starts jobs, reads results, installs skills within policy.
- **The operator:** an individual developer or small team who sets policy once and approves irreversible actions.
- **The skill or plugin author:** publishes once and gets automated verification instead of a human gate.

## Requirements

- **Containment:** no credentials and no network inside a sandbox, only a channel to the broker; best-available isolation, recorded per job. v1 runs on Linux hosts and uses provider API keys held by the broker. ([ADR-0002](../adr/0002-one-binary-two-processes-sandboxes.md), [ADR-0011](../adr/0011-pluggable-substrates.md), [ADR-0016](../adr/0016-agent-runtime-and-broker-protocols.md))
- **Authority:** every job runs under a signed manifest; the broker enforces it and nothing else. ([ADR-0004](../adr/0004-one-signed-manifest-per-job.md))
- **Provenance:** every value carries its source; workflows that let tainted text reach an actor fail to install. ([ADR-0005](../adr/0005-provenance-is-a-type.md))
- **Fail closed:** every gate answers allow, deny or unknown; unknown holds the task with a decision. ([ADR-0009](../adr/0009-gates-answer-allow-deny-unknown.md))
- **Record:** every state change is a signed, hash-chained event; the log is the database, the audit trail and the backup. ([ADR-0003](../adr/0003-event-log-is-the-database.md))
- **Ecosystem:** Agent Skills, WASM components and MCP servers run under declared capabilities; updates that add a capability never install silently. ([ADR-0006](../adr/0006-plugins-are-skills-and-wasm-components.md), [ADR-0007](../adr/0007-capability-based-marketplace-trust.md))

## Reference job

Every requirement above should be traceable through one real job: "From Claude Code, I ask Hyalos to fix issue #42 in my GitHub repo and open a PR." The walk, with the ADR or story behind each step, is in [`docs/planning/reference-job-walk.md`](../planning/reference-job-walk.md).

## Success metrics

| Metric | Target |
| --- | --- |
| Install to first contained job | Under 10 minutes on a clean machine |
| Trusted core size | At most 30,000 lines |
| Red-team corpus reaching a gated sink (worst case, offline) | Below today's 35% baseline, target set after the first measurement |
| Irreversible actions with human approval | 100% |
| Jobs run on the best isolation the host offers | 100%, recorded per job |
| First-pass merge rate | No worse than today's hivrr by more than 10 points |

## Milestones

| Milestone | Epics | Outcome |
| --- | --- | --- |
| M1 | E1 Event log and engine loop; E2 Substrates and the first contained job; E3 Broker and manifests | Proves the boundary: a real harness (Claude Code or Codex) in a sandbox with no secrets fixes a given issue and opens a PR, driven from the CLI |
| M2 | E4 Provenance, roles and the workflow language; E5 Agent interface and approvals | Safe for an agent to drive: the full reference job above runs through MCP |
| M3 | E6 Plugin runtime; E7 Marketplace and supply chain | Open to skills and plugins |
| M4 | E8 Verification and release gates; E9 Learnings and self-improvement; E10 Launch | Public release |

Epic and story detail is tracked in GitHub issues: each epic is a parent issue (#1, #8, #15, #22, #28, #35, #41, #49, #56, #60) with its stories as sub-issues, grouped by milestone.

## Risks

- **Rust velocity:** slower iteration than Python or Go. Mitigation: a small core, strong types, agent-written code under human review on the trusted paths.
- **Standards still moving:** MCP and WASI revise yearly. Mitigation: isolate protocol code behind adapters, track conformance suites.
- **Scope creep from today's hivrr:** the temptation to rebuild every feature. Mitigation: the non-goals above, and the ADR process for anything new.
- **Solo bandwidth:** one maintainer. Mitigation: milestones that each end in a demo, and an external review before M4.

## Open questions

Tracked in [`docs/planning/open-questions.md`](../planning/open-questions.md). They are facts to confirm by running spikes, and each is also an issue.

## References

- Cursor marketplace incident, October 29, 2026: BleepingComputer, "Malicious Claude Code extension on Cursor planted SSH backdoors."
- Google Cloud Agent Compute Engine, announced October 22, 2026: coverage at techjacksolutions.com and tech-insider.org.
- MCP specification, revision 2026-07-28: modelcontextprotocol.io.
- WASI 0.3 release, June 11, 2026: wasi.dev/releases/wasi-p3.
- Agent Skills open standard: agentskills.io.
- Today's hivrr: the gap analyses, PRD-109 to PRD-112, the threat model and the red-team baseline.
