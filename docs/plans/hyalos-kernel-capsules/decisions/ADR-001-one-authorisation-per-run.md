# ADR-001: One authorisation per run, naming the exact task

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** developer

## Context

The pack brief asked how a contract meant to run on new inputs could have a looser authorisation without reopening the re-roll attack. Record ADR-002 names exactly one task, inputs included, and left a looser form to a later pack if a real job needs one. No job needs one: the only workload is the issue digest, and a recurring job is authorised per run.

## Options

1. **A looser authorisation under a new extension version.** The runtime and script stay fixed and only the data varies. Rejected: the operator chooses the data, so even a budget of one lets them pick which input gets the attempt, which is record ADR-002's rejected option under another name, and no job needs it.
2. **One authorisation per run, as record ADR-002 built it.** Chosen.

## Decision

A customer authorises one exact task identity, contract, SDK, script and data included, for each run. This pack adds no authorisation form.

## Consequences

The attempt budget and ticket rules (record ADR-004, ADR-008, ADR-010) apply unchanged. A recurring job asks for one authorisation per run. A looser form, if a real job ever needs one, is a later pack's decision and a new ADR.
