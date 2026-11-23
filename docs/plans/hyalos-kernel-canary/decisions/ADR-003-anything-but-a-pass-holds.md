# ADR-003: Anything but a pass holds the notary

**Status:** accepted (developer, 2026-11-22); amended by ADR-007, a directory that refuses both rename and removal is a stated limit

**Proposed by:** author (spec); accepted by the developer at the spec acceptance checkpoint

## Context

#409 asks to hold on anything other than the expected refusal. An outage says nothing about the provider's behaviour, but a softer rule would let a check that could not conclude leave a pass in place.

## Options

1. **Every completed check that is not a pass replaces the state with no pass.** Chosen.
2. **An inconclusive check keeps an earlier pass until its 24 hours end.** Rejected by the developer at the acceptance checkpoint.

## Decision

A completed check that is not 200, 400, 200 leaves no pass. A check that crashes before its verdict changes nothing, so an earlier pass still ages out within its window.

## Consequences

A provider outage during a check stops runs until a later check passes.
