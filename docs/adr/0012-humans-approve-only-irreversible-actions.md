# ADR-0012: Humans set policy and approve only irreversible actions

**Status:** Proposed. The original open question on the verifier agent for escalations was settled in the November 1, 2026 planning session; folded in below.

## Context

Per-action human approvals don't scale and push people to approve without reading. The goal is an agent that can't get you into trouble.

## Decision

Three tiers.

- **Routine** (a job within its manifest, an install within the operator's install policy): decided by policy.
- **Escalated** (a single job that needs a host outside the allowlist): deterministic checks plus an independent verifier agent that shares no context with the requester, optionally a human by policy.
- **Irreversible** (changing policy, publishing publicly, merging to a protected branch, spending above a cap, exporting credentials): always a human.

**A capability grant always needs a human.** That covers a new install that declares capabilities beyond the operator's policy, and an update that adds a capability beyond it. A verifier agent can never approve a capability grant, or anything touching credentials, policy or publishing. An LLM verdict alone never approves any of these.

In v1 the operator approves with `hyalos approve <id>` on the machine, signed with a local key that the driving agent's MCP credential cannot use. Passkeys come later.

## Consequences

Human attention goes where it matters. This closes the gap between [ADR-0007](0007-capability-based-marketplace-trust.md) ("waits for consent") and the verifier: consent means a human, so the PRD's "never installs silently" holds. Because the operator's install policy defines what is pre-approved, changing that policy is an irreversible action and is where capability breadth is really decided.
