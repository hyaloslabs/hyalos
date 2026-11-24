# ADR-006: This pack ends at a ready review package

**Status:** accepted

**Proposed by:** developer

## Context

The parent gives this pack an independent review (#63, #412). An agent can prepare for a review, but only an outside person can perform one, on a schedule this pack cannot control. Capsules depends on this pack.

## Options

1. **Include the review.** The final slice stays blocked until a reviewer's report exists and every finding is closed. Rejected: capsules could wait a long time on an outside party.
2. **End when the review package is complete and verified, and track the engagement and its findings in a separate issue.** Chosen.

## Decision

This pack is done when the review package is complete and its commands are shown to work from a clean checkout. Engaging a reviewer and closing findings are tracked in a separate issue.

## Consequences

- Nothing may claim hyalOS has been independently reviewed when this pack finishes.
- #63 stays open for v1's trusted core. Publishing this pack adds the separate hyalOS review issue, and the parent's `coverage.md` row points to it.
- A later finding may change this pack's work, through a new issue and ADR.
