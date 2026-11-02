# ADR-0004: One signed manifest per job, enforced only by the broker

**Status:** Proposed

## Context

Claims, virtual keys, job tokens, scoped git tokens and per-route ACLs each encoded part of a job's authority.

## Decision

The engine signs one manifest per job: repo and branch prefix, actions, egress hosts, tools, data flows, budget, deadline and taint policy. The broker enforces that document and nothing else. Forge access uses per-job tokens (GitHub App installation tokens, GitLab project tokens) obtained by exchange; no operator PAT exists.

## Consequences

Authority is readable in one place and diffable between jobs. The manifest schema is the most important interface in the system and is versioned with care.
