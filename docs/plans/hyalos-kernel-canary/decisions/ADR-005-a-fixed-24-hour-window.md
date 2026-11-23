# ADR-005: A fixed 24-hour window, enforced by the notary and the audit

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec acceptance checkpoint

## Context

#409 asks for a check before each run or at least daily. Checking before every run would spend the customer's quota and need the credential at hand for every run.

## Options

1. **A fixed 24 hours, checked at every session and again by the audit.** Chosen.
2. **A check before every run.** Rejected: cost, and a credential in constant use.
3. **A window the customer sets.** Rejected: it would need signing, and a long window would weaken what the report means.
4. **Checking freshness only at a run's first session.** Rejected: a long run could outlive its pass.

## Decision

A pass covers a session when it is not later than the session's signed time and at most 86,400 seconds older. The boundary itself is fresh.

## Consequences

A run that crosses the window ends refused. A shared test pins the notary's and the monitor's constants to the same value.
