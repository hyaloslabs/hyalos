# ADR-009: Every contract task's report states Python ADR-008's limit

**Status:** accepted (developer, 2026-11-25); amended by ADR-025, a pre-pin default build reads version 5 as inconclusive, refused after the pin

**Proposed by:** developer

## Context

Python ADR-008 requires a Python task's audit report to say the record proves what the runtime computed, not that the script ran as its source reads. The record names only a profile and a guest hash, so the audit cannot tell a runtime guest (probe 9). The golden report needs a record that passes audit.

## Options

1. **Detect a runtime guest from the contract or a new record field.** Rejected: a check that can be dodged by naming the guest differently.
2. **Every new-version report states the limit.** Chosen: it is true of any task that computes from its inputs.
3. **A real notary-attested record for the golden report.** Rejected: it ties a pinned test to the real provider.

## Decision

Every passing record of the new version gets the limit's statement in its report. Version 1 to 4 reports are unchanged. The golden contract-task record passes audit under the `test-trust` stand-in log and provider in a test build only, and a release `hyalos-verify` refuses it.

## Consequences

The golden reports gain new-version entries; existing ones stay pinned.
