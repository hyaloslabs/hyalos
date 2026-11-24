# ADR-004: Name the guest interface outside the record

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

Capsules and the Python spike will build on the four guest calls, which today are defined by a short document and by two implementations (`spikes/task-kernel/ABI.md`). The verifier pack is introducing record version 4 with `hyalos.profile/1` in the task identity (verifier ADR-002). The interface needs a name and tests. This pack's scope excludes changing the record format.

## Options

1. **Add the interface version to the record now.** Rejected: it would need another record version while the verifier pack is still building version 4, and a guest's imports already appear in its signed bytes.
2. **Fold it into the execution profile.** Rejected: the profile belongs to the verifier pack (verifier ADR-007), and it describes interpreter settings rather than the host calls.
3. **Leave it unnamed and rely on the document.** Rejected: nothing would stop a silent change.
4. **Name it `hyalos.abi/1` in the document and a shared constant, and hold it with a conformance suite on both backends.** Chosen.

## Decision

The guest interface is `hyalos.abi/1`. Its calls, signatures, limits and error values are held by a conformance suite on both backends. Any change fails that suite until the version and the document change. The record does not carry the name.

## Consequences

- Capsules gets a stable target, and a later interface version is an explicit decision.
- A record cannot state which interface version it used. If a second version ships, that version's pack must add it to the record.
