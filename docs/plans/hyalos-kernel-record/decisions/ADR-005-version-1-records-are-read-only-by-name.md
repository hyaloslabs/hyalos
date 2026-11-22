# ADR-005: Version 1 records are read only when asked for by name

**Status:** accepted (developer, 2026-11-22); amended by ADR-011, a version 1 audit refuses ticketed attestations and a legacy pass reads differently

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

Real version 1 chains and records are committed in the repository and must stay checkable. A version 1 record has no ticket, so if the audit accepted it by default, an operator could avoid the budget by presenting their work in the older format.

## Options

1. **Accept version 1 silently.** Rejected: it is a downgrade path around the whole budget.
2. **Stop reading version 1.** Rejected: it would orphan the real evidence the earlier packs produced.
3. **Rewrite version 1 records to version 2.** Rejected: their commitments are in the public log and cover their exact bytes.
4. **Read version 1 only with `--legacy-v1`, and state in the report that it has no attempt budget.** Chosen.

## Decision

Without `--legacy-v1` the audit refuses a version 1 record and names the option. With it, the old checks apply and the report says the record has no attempt budget. Replay of a version 1 record stays byte-identical.

## Consequences

- An auditor cannot be led into the older rules by the record alone.
- The monitor must rebuild a record at the version it was given when replaying, rather than always writing the newest.
- An attestation from before this pack remains valid under the same notary key, so the legacy statement in the report is what protects a reader.
