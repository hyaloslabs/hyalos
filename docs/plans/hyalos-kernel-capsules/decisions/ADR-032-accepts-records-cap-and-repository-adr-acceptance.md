# ADR-032: The `accepts.records` cap, and when the pack's repository ADRs are accepted

**Status:** accepted (developer, 2026-11-27)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 4's re-check

## Context

This amends ADR-013 and ADR-022. ADR-013 caps `accepts.records` at 4,096, but a framed inputs file holds at most 4,096 records in all, counting the contract and up to 2 code records, so at that cap one data record too many is refused `BadInputs` rather than `InputLimit`. ADR-022 has the pack's repository ADRs (0138 to 0140) written Proposed, while the repository's rule (`CLAUDE.md`) sets a feature's ADR to Accepted in its pull request's last commit before it merges; ADR-031 applied that rule to 0140 only.

## Options

1. **Keep 4,096** and accept `BadInputs` at the top. Rejected: the boundary example would not reach the contract's own refusal.
2. **Cap `accepts.records` at 4,092.** Chosen.
3. **Apply the acceptance rule to 0140 only.** Rejected: it goes against the repository's rule.
4. **Apply it to 0138, 0139 and 0140.** Chosen.

## Decision

- `accepts.records` is 0 to 4,092: the 4,096 framing cap less the contract, 2 code records and the one record over, so `accepts.records` + 1 data records always frame and are refused `InputLimit`. The corpus edge files are 4,092 (`good/`) and 4,093 (`bad/`). ADR-013's other bounds are unchanged.
- ADR 0138 (Slice 2), 0139 (Slice 8) and 0140 (Slice 10) are each written Proposed and set to Accepted by the developer, with its README row, in its slice's last commit before the pull request merges.

## Consequences

Every contract's boundary example reaches `InputLimit`. Every repository ADR the pack adds is accepted before its slice merges.
