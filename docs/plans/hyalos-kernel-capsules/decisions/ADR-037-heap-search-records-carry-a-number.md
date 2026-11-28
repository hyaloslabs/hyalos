# ADR-037: The heap search's records carry a number, so every record is asked

**Status:** accepted (developer, 2026-11-27)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 8

## Context

This amends ADR-033. ADR-033's outcome search builds each record as "a fixed ASCII title padded with `a` to the size tried". The triage script reads a record as `<number>\t<title>` and counts a record with no tab as `other` without asking (`spikes/438/sdk/draft.py:43-45`, `spikes/438/scripts/triage.py:18-21`). A title-only input therefore asks no question, so the search would never hold a question and its answer in the heap, and the per-attestation figures measured on that input would not exercise the model. The spike's committed heap case carries a number and a tab (`spikes/438/evidence/data/hostile-heap.tsv`).

## Options

1. **Title-only records, as ADR-033 says.** Rejected: no record is asked, so the search measures memory without the questions the task really holds.
2. **Each record is `<n>\t` and a padded title, exactly the size tried.** Chosen.

## Decision

- Each search record, and each record of `measure-accepts.sh input`, is `<n>\t` (`n` from 1 to the number of records, in decimal) followed by a fixed ASCII title padded with `a`, so the whole record is exactly the size tried and every record is asked.
- `record_bytes` counts the `<n>\t` prefix. The title the guest asks is clipped to 4,096 bytes by `TITLE_MAX` (`hyalos-os/guest/src/lib.rs:30`, and the script's own), so a record longer than 4,096 bytes is held whole in the heap but asked clipped. ADR 0139 says so.
- Everything else in ADR-033 holds: the search order, the margin, the 512-byte stop, and the run at the declared limits on both backends.

## Consequences

The search exercises the task as it runs on real input: every record is asked and answered, so R_max reflects the heap with each question and answer held.
