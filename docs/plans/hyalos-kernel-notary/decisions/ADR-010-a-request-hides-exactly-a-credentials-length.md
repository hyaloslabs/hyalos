# ADR-010: A request hides exactly a credential's length

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (issue #394); chosen by the developer on 2026-11-21

## Context

An attested request hides one run of bytes, the credential's value. Verify sees that run's length and never its content. With a loose bound on the length, a cheating operator could hide a line break after the credential, then a body of their own. The provider would answer that body while verify reported the approved title and binding. This matters once the notary is not the operator's (Slice 2).

## Options

1. **Prove the hidden bytes hold no line break.** Rejected for now: it needs a proof over hidden bytes that this pack's use of the protocol library does not make.
2. **Name it as a limit in the audit's report.** Rejected: it would leave a known forgery in a system meant to bind the operator.
3. **Require the hidden run to be exactly the credential's length.** Chosen.

## Decision

The hidden run must be exactly 106 bytes, the length measured from the public presentations of real runs. The number is a constant in the tool's source, not a setting or an input, because the party being checked must not be able to choose it. Verify refuses any other length. Ask refuses a credential of any other length before it sends anything.

## Consequences

- The hidden run must start with a working credential, so a credential of the pinned length leaves no room for anything else.
- **Residual.** An operator who holds a working credential shorter than 106 bytes could use the spare bytes. This decision assumes the provider issues none for this kind of credential. That assumption was not tested.
- A credential of another length, including an API key if its length differs, is refused until the constant changes. Changing it is a reviewed code change.
- Slice 2 moves the constant to the library file with the other shared constants.
