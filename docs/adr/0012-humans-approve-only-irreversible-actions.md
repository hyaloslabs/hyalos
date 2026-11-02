# ADR-0012: Humans set policy and approve only irreversible actions

**Status:** Proposed. Depends on [open question 2](../planning/open-questions.md) (verifier agent for escalations).

## Context

Per-action human approvals don't scale and push people to approve without reading. The goal is an agent that can't get you into trouble.

## Decision

Three tiers. **Routine** (a job within its manifest, an install within policy): decided by policy. **Escalated** (an update adding a capability, a host outside the allowlist): deterministic checks plus an independent verifier agent that shares no context with the requester, optionally a human by policy. **Irreversible** (changing policy, publishing publicly, merging to a protected branch, spending above a cap, exporting credentials): always a human.

## Consequences

Human attention goes where it matters. An LLM verdict alone never approves an irreversible action.
