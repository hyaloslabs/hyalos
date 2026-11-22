# ADR-009: Notary state moves forward once, and a rollback retires the authorisations

**Status:** accepted (developer, 2026-11-22)

**Proposed by:** author (spec), from the pack's adversarial read (F5); accepted by the developer at the spec ADR checkpoint

## Context

The draft design let a customer roll back by restoring the old sequences file. The authorisation files would survive and the spent tickets would not, so the next run would receive ticket 0 again, validly signed: one ticket issued to two runs.

## Options

1. **Keep the restore, and warn in the documentation.** Rejected: an operator can ask for exactly this after claiming the upgrade broke something.
2. **Keep a separate, append-only ticket journal.** Rejected: a customer restoring a directory restores that too, so it adds a file without closing the case.
3. **Migrate before serving, refuse a version 1 file once any authorisation exists, and make rollback retire every authorisation.** Chosen.

## Decision

The notary rewrites a version 1 sequences file as version 2 at start, before it serves. `notary authorize` requires a version 2 file. A notary that finds a version 1 file while any authorisation exists refuses to start. The documented rollback moves to a new notary directory and key, which retires every authorisation.

## Consequences

- An upgrade cannot be undone in place.
- A customer who restores a whole directory from backup can still return spent tickets. The notary cannot detect that, so `RECORD.md` states it as the customer's own responsibility, under the existing limit on the notary's independence.
