# ADR-008: Only a task's newest authorisation takes new runs, and each carries the budget before it

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec), from the pack's adversarial read (F2, F3, F4); accepted by the developer at the spec ADR checkpoint

## Context

This amends ADR-001, whose Consequences say an old attestation cannot understate anything. With `--again` it can. An operator who gets a second authorisation after a claimed outage can publish the record from the first and show "attempt 1 of at most 1". While earlier authorisations stay live, they can also spend the new one and then start a run under the old one.

## Options

1. **Remove `--again`.** Rejected: a provider outage would leave a task permanently unable to run on that notary.
2. **Publish every authorisation to the public log, so an auditor sees later ones.** Rejected: it brings back the log identity and freshness rule ADR-001 declined, for a case the report can state honestly.
3. **Only the newest authorisation takes new runs, each signs the total budget of the rounds before it, and the report states what it cannot show.** Chosen.

## Decision

A new run may spend a ticket only on the task's highest round. Otherwise the notary answers `authorisation`. Each authorisation records the sum of earlier rounds' budgets, and the ticket extension signs it. The report says `at most N under this authorisation, P authorised earlier`, names the identity as this exact task on this notary, and adds the limit `not proven: the customer did not authorise this task again after this attestation`. `notary tickets --dir <d>` shows the customer every authorisation and how many tickets it has spent.

## Consequences

- A later round is visible in every attestation made under it. An earlier record cannot show rounds that came after it, and the report says so.
- A changed guest or input is a different task with its own rounds. Refusing custom sections would not stop this, since any harmless code change has the same effect, and the shipped guest carries three such sections. The customer decides what they authorise.
- A run already holding a ticket on an older round may finish.
