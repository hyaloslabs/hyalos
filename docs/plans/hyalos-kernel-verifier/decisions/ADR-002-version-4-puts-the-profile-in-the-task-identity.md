# ADR-002: Record version 4 puts the profile in the task identity

**Status:** accepted (developer, 2026-11-23)

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The parent's ADR-002 requires each new record to name its execution profile. A customer's authorisation names one exact task identity (record pack ADR-002), and today that identity covers only the guest and its inputs. If the profile sat outside it, one authorisation would cover the same task under different limits or interpreter settings.

## Options

1. **Add a `profile` field to the record only.** Rejected: the customer's authorisation would not bind the profile.
2. **Identify the profile by a hash of the interpreter's binary.** Rejected: the process and bare-metal backends are different binaries that must produce the same record.
3. **A new framed identity, `hyalos.task/4`, that includes the profile's name.** Chosen.

## Decision

- Version 4 adds a required `profile` field to the transcript. The only known value is `hyalos.profile/1`.
- The version 4 task identity is the SHA-256 over the tag `hyalos.task/4`, the profile name's length as 8 big-endian bytes, the profile name, then the guest hash and inputs framed exactly as in version 2.
- Links carry `v: 4`. Every rule that version 3 applies, including the required canary, applies to version 4.
- Versions 1 to 3 carry no `profile`, replay under `hyalos.profile/1`, and are refused if they hold the field.

## Consequences

- The notary treats a task as an opaque 64-character hash, so it needs no change.
- A customer must authorise the version 4 identity. `task-kernel task` prints the version 2 identity today (`spikes/task-kernel/kernel/src/main.rs:1087`) and must change to print the version 4 one, or a customer would authorise a task no new run can use.
- Any change to a profile's contents needs a new profile name, and the old one stays supported.
- New runs are version 4 once this ships, so the committed real run stays version 3 until a new real run is made.
