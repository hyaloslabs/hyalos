# ADR-002: Multi-step work is one task's script; no signed chains

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

The pack brief listed signed chains in scope. Under one authorisation per run (ADR-001), a later step's inputs are an earlier step's output, so a chain of separately authorised tasks cannot be authorised in advance. Parent ADR-003 makes v1's engine the one control plane for work that spans steps.

## Options

1. **Signed chains as a construct of their own.** Rejected: each step would need an authorisation the customer cannot give in advance, and between steps the answer passes through the operator, the party the record defends against. It also makes a second orchestrator beside v1's engine.
2. **Steps inside one task's script, under one authorisation and one record; work that spans tasks is the engine pack's workflow.** Chosen.

## Decision

Multi-step work is one task's script. This pack defines no chain format. Work that spans tasks, such as waiting days for a person, is v1's workflow under parent ADR-003, each task authorised when it starts.

## Consequences

One ticket budget and one replay cover every step. A long sequence is one larger task, so the question limit (ADR-007) and fuel bound it. The engine pack owns cross-task workflows.
