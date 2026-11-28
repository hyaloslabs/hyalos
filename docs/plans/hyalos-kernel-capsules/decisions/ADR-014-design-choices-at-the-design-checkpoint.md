# ADR-014: Design choices settled at the design checkpoint

**Status:** accepted (developer, 2026-11-25); amended by ADR-016, a lower ceiling needs version 6; amended by ADR-017, a per-schema prompt replaces the fixed system line; amended by ADR-018, the test hooks are kept out of release by the release verifier's trust

**Proposed by:** author (spec); accepted by the developer at the spec design checkpoint

## Context

The Architect's draft left ten choices open (design.md DR1, DR4 to DR10, DR12). Each had a proposal and at least one rejected option.

## Options

1. **The digest guest:** (rejected) keep `digest.wasm` and count the contract as one `other` issue, a wrong digest; (rejected) hide record 0 from the guest, against S-03.1; (chosen) a version 5 digest guest that reads data from record 1, beside today's `digest.wasm`, which stays byte for byte.
2. **The model's instruction:** (rejected) a per-schema `prompt` in the contract, text the notary cannot tie to the record; (chosen) one fixed system line for every schema.
3. **The deadline's call time and count:** (rejected) per-deployment call time; (rejected) counting every record, which breaks S-08.3.e3; (chosen) the slower measured path, and data records only for version 5.
4. **Test hooks:** (rejected) a `test-trust` Cargo feature on `attest`; (chosen) `standin --answer` and `--delay` options, kept out of release because a default build trusts no stand-in root, shown by the release-switch test.
5. **A ceiling below 200:** (rejected) check the ceiling only in run commands; (chosen) the parser checks it everywhere, no version 5 record leaves the pack before S-08.2, and the ceiling slice re-pins.
6. **The generator:** (rejected) stay in `spikes/438/`, run by hand; (chosen) `hyalos-os/generate/`, run by a test in CI over the shared corpus.
7. **The answer field:** (rejected) rename to `value` in version 5; (chosen) keep `label`.
8. **Line-form inputs:** (rejected) accept both forms by magic; (chosen) framed only, with fixtures written through `frame --lines`.

## Decision

Each chosen option above is the design: design.md §1 to §2 state them as contracts.

## Consequences

Two committed digest guests exist; `hyalos-os/review/README.md:33` is updated in the slice that adds the second. Whether the provider honours the closed-type schemas (DR11) is not a decision: the measurement slice probes it, and risk R3 covers it.
