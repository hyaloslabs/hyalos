# ADR-010: A request hides exactly a credential's length

**Status:** accepted (developer, 2026-11-21); a body forged in the hidden bytes is tested in ADR-011, and the API-key case in its residual is tracked in #398

**Proposed by:** author (issue #394); chosen by the developer on 2026-11-21

## Context

An attested request hides one run of bytes, the credential's value. Verify sees that run's length and never its content. With a loose bound on the length, a cheating operator could hide a line break after the credential, then a body of their own. The provider would answer that body while verify reported the approved title and binding. This matters once the notary is not the operator's (Slice 2).

## Options

1. **Prove the hidden bytes hold no line break.** Rejected for now: it needs a proof over hidden bytes that this pack's use of the protocol library does not make.
2. **Name it as a limit in the audit's report.** Rejected: it would leave a known forgery in a system meant to bind the operator.
3. **Require the hidden run to be exactly the credential's length.** Chosen.

## Decision

The hidden run must be exactly 106 bytes, the length measured from the public presentations of real runs, all of which use the `authorization: Bearer` head. The number is a constant in the tool's source, not a setting or an input, because the party being checked must not be able to choose it. Verify refuses any other length and any other head, including the `x-api-key` head. Ask refuses an API key, and a credential of any other length, before it sends anything.

## Consequences

- A request that hides more than 106 bytes, or fewer, is refused, so a working credential of the full length leaves no room for anything else.
- **Residual.** Verify never sees the hidden bytes, so it cannot tell that they are the bearer credential. Any credential the provider accepts, of any kind and in any header, that fits in 106 bytes with its framing leaves room to alter the request. For example, the run could be a junk bearer value, a line break, and `x-api-key: <key>`, which fits an API key of up to 92 bytes. Whether such a key exists, and how the provider treats a bad `authorization` beside a good `x-api-key`, was not tested. Until they are, this decision narrows the forgery and does not rule it out. Tracked in #396.
- A different credential is refused until the constant and the accepted head change. Changing them is a reviewed code change.
- Slice 2 moves the constant to the library file with the other shared constants.
