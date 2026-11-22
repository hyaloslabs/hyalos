# ADR-005: The audit relies on the public log's signed time, not the Bitcoin timestamp

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (spec); accepted by the developer at the spec acceptance checkpoint

## Context

The spike asks a Bitcoin timestamp calendar to anchor each result link and keeps the pending proof without ever verifying it. The parent pack listed that as a gap. Binding an answer to its run (ADR-002) needs a time for each link that an auditor can verify.

## Options

1. **Verify the Bitcoin timestamp in this pack.** Rejected: a proof is only complete hours after the run, once a block confirms, so an audit run soon after would have nothing to check. It also adds a second outside service to the audit.
2. **Use the time the public log signs for each entry.** Chosen.

## Decision

The audit takes each link's time from the public log's signed entry and verifies that signature. The Bitcoin timestamp is still requested and kept, and is not verified in this pack.

## Consequences

- The audit's notion of time depends on the public log's key. An auditor who distrusts the log has no second source yet.
- Verifying the Bitcoin timestamp moves to `hyalos-kernel-record`, where a record is checked long after its run.
