# ADR-013: A provider session that fails before answering may be logged `dropped`

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** executor (Slice 2, independent review); accepted by the developer on 2026-11-21

## Context

`design.md` §2, How ask says why it refused, lists the notary's session line for ask's word `provider` as `server`, `limits`, `sequence` or `attested`. When the provider fails before it answers, the operator's side of the protocol stops. The notary sees only that the session ended early, and cannot tell why.

## Options

1. **Have the operator tell the notary why it stopped.** Rejected: the notary would be recording an unverifiable claim from the party it exists to check.
2. **Log such a session as `server`.** Rejected: the server's name was not refused, so the line would be false.
3. **Log it as `dropped`, which is what the notary observed.** Chosen.

## Decision

The `provider` row also allows the session line `dropped`. This adds to `design.md` §2 and changes nothing else in it. Ask's word stays `provider`.

## Consequences

- The session line records what the notary saw. Ask's word records what the operator saw.
- A `dropped` line alone does not say whether the provider or the link failed.
