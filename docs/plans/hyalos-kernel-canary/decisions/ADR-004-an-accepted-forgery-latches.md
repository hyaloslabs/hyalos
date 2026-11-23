# ADR-004: An accepted forgery latches until the customer resets it

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec acceptance checkpoint

## Context

A 200 on the forged shape means the assumption of notary ADR-011 was false at that moment. A later 400 does not show why, or that it will not return.

## Options

1. **Latch, cleared only by the customer's `canary reset`.** Chosen.
2. **Let the next pass clear it.** Rejected: an intermittent acceptance would clear itself unnoticed.

## Decision

A 200 on the forged shape writes `latched`. A later check keeps it. `canary reset` writes `held`, and a later pass then counts.

## Consequences

Reset is a customer decision, made after looking into why the provider answered. No agent or operator tooling runs it.
