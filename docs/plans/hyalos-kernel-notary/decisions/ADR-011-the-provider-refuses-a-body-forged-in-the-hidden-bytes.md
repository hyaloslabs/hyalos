# ADR-011: The provider refuses a body forged in the hidden bytes

**Status:** accepted (developer, 2026-11-21)

**Proposed by:** author (issue #396); chosen by the developer on 2026-11-21

## Context

ADR-010 requires a request to hide exactly 106 bytes, but verify never sees those bytes. A cheating operator could try to put a line break and a body of their own inside them. Verify pins the request's head, so its `content-length` is the approved body's. The provider would then read the forged body, the shown blank line, and the start of the approved body, cut off at that length.

## Options

1. **Prove in zero knowledge that the hidden bytes hold no line break.** Rules the forgery out whatever the provider does. It adds a proving system of about 1 GB of sources and a proof per model call. Kept as the fallback.
2. **Test what the provider does with the forged shape.** Chosen.
3. **Only name it as a limit.** Rejected while a test could answer it.

## Decision

The operator ran `spikes/task-kernel/attest/probes/hidden_header.py` against the real provider on 2026-11-21, with the bearer credential the tool uses.

| Request | Status |
|---|---|
| Control before: the smallest valid body alone | 200 |
| The same body, then the blank line and the start of the tool's approved body, up to the approved body's `content-length` | 400 |
| Control after: the smallest valid body alone | 200 |

The probe does not read the reply's body, so the 400's stated reason is not recorded. The same credential and small body were answered immediately before and after it, so the difference between the requests is the forged shape. An earlier run of a looser version of the probe, with one control and a stand-in body, gave 200 then 400. No verification code changes: the pins of ADR-010 stand, and this forgery is treated as refused.

## Consequences

- **The room is also tight.** The smallest valid body found is 86 bytes. With its blank line it fits in the 106 hidden bytes only beside a credential of 16 bytes or fewer. The credential in use fills all 106.
- **This holds for the provider's behaviour on the day tested.** The provider can change it. Re-run the probe before relying on this for a release; a 200 on the second request reopens option 1.
- **Chunked encoding does not fit.** Hiding `transfer-encoding: chunked` with the small body needs at least 124 bytes before any credential.
- **One forged shape was tested.** A shorter hidden prefix meant to use the shown bytes as the rest of its JSON was not sent. By the JSON grammar it should fail: the pinned length cuts off the approved body's closing brackets, and a raw line break is not allowed inside a string.
- **Residual, untested.** A short API key could be hidden beside a junk bearer value with the approved body unchanged. The question asked would be the same, but the call could run on another account or carry an added header. Testing it needs an API key, which the operator does not hold. It is not among the four limits S-04.5 names. Tracked in #398.
