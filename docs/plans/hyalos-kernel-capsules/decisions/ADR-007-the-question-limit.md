# ADR-007: The question limit: declared, capped, enforced by the monitor and the audit

**Status:** accepted (developer, 2026-11-25); amended by ADR-015, the notary signs blind and the verifier refuses free text

**Proposed by:** developer

## Context

Nothing bounds how many questions a run asks (context §2 E). The notary admits one session at a time and has no per-run cap. #411 asks for an explicit limit. A task that asks nothing never opens a session, and today's audit refuses a record with no attested answers.

## Options

1. **An implicit default limit for a contract that declares none.** Rejected: an undeclared bound is one the customer did not read.
2. **The notary also caps sessions at the contract's limit.** Rejected: the notary would have to read the task; under notary ADR-014 a session past the last answer cannot help a record pass, so the audit's count suffices (probe 7).
3. **Auditing a task that asks no question.** Rejected for this pack: it needs the customer's authorisation proven without a notary session, a new trust path.
4. **Every contract declares its limit within a ceiling; the monitor refuses the next question past it; replay and audit refuse a record past it; a notary and run of different versions are refused at session open.** Chosen.

## Decision

Every contract declares a question limit no greater than the ceiling, or it is refused `BadContract`. The ceiling is 200, provisional, until the #411 measurement replaces it (ADR-010). The monitor refuses question L+1 `QuestionLimit` before it leaves; replay and audit fail a record holding more answers than its limit. A session whose notary and run versions differ is refused when it opens. A no-question task spends no ticket and stays unauditable.

## Consequences

Upgrading the notary is the customer's step, written in the release notes. The question count belongs to the run, which fits a ticket spent at the run's first session (record ADR-004).
