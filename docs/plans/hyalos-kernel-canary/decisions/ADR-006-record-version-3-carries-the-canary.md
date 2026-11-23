# ADR-006: Record version 3 carries the canary

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec acceptance checkpoint

## Context

Version 2 records exist in the public chain and must keep their audits. If a version 2 audit simply accepted an optional canary, leaving it out would be a way around the check.

## Options

1. **A new version 3 that requires a canary on every answer, with a canary refused on any older version.** Chosen.
2. **An optional extension on version 2.** Rejected: a missing canary could not be told from a removed one.

## Decision

New runs write version 3. Version 3 requires one signed canary per answer. Versions 1 and 2 refuse any attestation carrying one. A version 2 report states it has no provider canary.

## Consequences

The task identity and intent link keep their version 2 forms, under `v: 3`. `RECORD.md` defines the new version.
