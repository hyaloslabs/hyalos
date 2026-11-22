# ADR-012: The hidden bytes are the fifth limit

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (issue #398); chosen by the developer on 2026-11-21

## Context

Verify never sees the 106 hidden bytes of an attested request. ADR-011 showed the provider refuses a body forged inside them. That refusal is conditional, as ADR-011 states: one forged shape was sent, on one day, and the probe must be re-run before a release, where a 200 reopens the zero-knowledge option. Its named residual is one case: a junk bearer value, a line break and `x-api-key: <key>`, with the approved body unchanged. The question asked would be the one shown, but the call could run on another account or carry an added header. Testing it needs an API key, which the operator does not hold.

## Options

1. **Test it with an API key.** Rejected for now: the operator holds none.
2. **Prove in zero knowledge that the hidden bytes hold no line break.** Rejected for now: it adds a proving system of about 1 GB of sources and a proof per model call, to close a case that does not change the question asked.
3. **Name it as a limit in what the audit reports.** Chosen.

## Decision

S-04.5 gains a fifth limit, stated by every report that relies on an attestation: the request's hidden bytes are not shown, so the call may have used another credential or carried an added header.

The monitor's attestation check prints this line today, from the constant `HIDDEN_BYTES_LIMIT` in `spikes/task-kernel/kernel/src/main.rs`. Slice 4's report lists it with the other four and uses that constant. This decision replaces the sentence in ADR-011 that says the case is not among S-04.5's limits.

## Consequences

- An audit does not claim that a call ran on a particular account, or that it carried no added header.
- The gap stays open to anyone holding a working credential, of any kind and in any header, short enough to fit in the 106 hidden bytes with its framing (ADR-010). A later test or proof that closes it removes this limit by a new decision.
