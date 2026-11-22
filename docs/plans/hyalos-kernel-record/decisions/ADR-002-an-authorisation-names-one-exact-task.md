# ADR-002: An authorisation names one exact task, guest and inputs

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

An authorisation has to say what its attempts are for. If it is loose, the operator can choose among inputs inside the budget and keep the one whose answer they prefer, which is the re-roll #408 exists to stop.

## Options

1. **The guest only.** Any inputs may run under the budget. Rejected: the operator chooses the inputs, so a budget of 1 still lets them pick which input gets the one attempt. It suits recurring jobs with fresh data, but those can be authorised per run.
2. **A pattern or schema of allowed inputs.** Rejected: it needs a policy language this pack has no use for, and a loose pattern reopens the same choice.
3. **The exact task: the guest and its inputs, by the version 2 task identity.** Chosen.

## Decision

An authorisation names one version 2 task identity. The customer computes it with the `task` command from the guest and inputs. The audit refuses a record whose recomputed task is not the one the notary signed.

## Consequences

- The customer must know the inputs before authorising. A recurring job needs one authorisation per run's inputs.
- The notary learns only a hash of the task.
- A later pack can add a looser form under a new extension version if a real job needs it.
