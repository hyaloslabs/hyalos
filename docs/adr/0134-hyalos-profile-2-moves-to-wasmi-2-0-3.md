# ADR-0134: `hyalos.profile/2` moves to `wasmi` 2.0.3, and profile 1's records still replay

**Status:** Proposed

## Context

hyalOS pins `wasmi` at `=2.0.2` in the monitor, the audit library and the kernel image, and the execution profile `hyalos.profile/1` names that version (`hyalos-os/profile/src/lib.rs`, `hyalos-os/RECORD.md`). Replay compares the whole transcript byte for byte, including `fuel_used`, so the interpreter's version is part of what a record means.

On 2026-11-24 wasmi-labs published GHSA-qrr7-2g7f-r9rg (high), "store{8,16}_lane decodes incorrectly without the memory64 feature", for `wasmi` and `wasmi_ir` from `2.0.0-beta.4` to `2.0.2`. `2.0.3` fixes it, and crates.io has yanked `2.0.2`. Once RustSec lists it, `cargo deny` fails CI's `hyalos-os` job. The developer decided to move to `2.0.3`.

The pack ADRs that define the profile bind this change. ADR-002 of `docs/plans/hyalos-kernel` (Accepted) says a profile's contents never change, that a change is a new profile, and that a security fix in `wasmi` needs a new profile and a decision on whether records from the old one are still accepted. ADR-007 of `docs/plans/hyalos-kernel-verifier` (Accepted) gives that pack the definition. ADR-002 also says a verifier keeps the old profiles so old records still replay, and expects each profile a verifier supports to add interpreter code to it. Keeping profile 1's interpreter, a second, yanked and vulnerable `wasmi`, in every build is not an option, so this ADR adds a narrower rule: an old profile's records replay on a newer `wasmi` that was checked equal to its own. This refines ADR-002 and ADR-007 and supersedes no Accepted ADR. ADR-0130 left `wasmi` unchanged while merging the lockfiles. This change comes after that and does not change that decision.

What changed from `2.0.2` to `2.0.3`, read from the published crates:

- `wasmi_ir`: the store-lane operator's `offset` field becomes `Offset`, under `#[cfg(feature = "simd")]`. This is the advisory's fix.
- `wasmi`: a test of it, also under `simd`. `Error` now holds a trap code as a pointer to a `static` instead of a box, so a trap no longer allocates. `Break` and `TrapCode` are generated from one list, with the same variants and values.
- `wasmi_core` and `wasmi_collections`: the trap-code list as a macro, and the versions.

Every hyalOS build has `simd` off and `memory64` on, and the profile refuses SIMD, so the fix is in code our builds do not compile. Nothing changes fuel metering, translation, validation or execution of an instruction a profile 1 guest can use. The crate features are unchanged (`cargo tree -e features -i wasmi`): the kernel image enables `stable`, `validate`, `memory64`, `auto-dispatch` and `libm`, and the host builds take `wasmi`'s defaults, which include `memory64` and not `simd`.

## Decision

- Add `hyalos.profile/2`: profile 1's settings and limits, unchanged, on `wasmi` `2.0.3`. New runs name it, and the kernel image is built with it. Profile 1 stays defined as it was, naming `2.0.2`.
- A record replays only on the `wasmi` its profile names, or on one written into the profile crate's `CHECKED_EQUAL` list next to the linked version. A profile whose `wasmi` is neither is not known to this build, and a record naming it is refused as an unknown profile. This holds for records of versions 1 to 3 as well, which run under profile 1 without naming it.
- `("2.0.2", "2.0.3")` is the one pair on that list. Records under profile 1 are still accepted and replay on `2.0.3`. Equality was checked three ways for this change: the diff above, the digest guest's transcripts and fuel for seeds 0 and 7 on both backends (byte-identical to `main`'s `2.0.2` builds, and still matching the hashes pinned in `core_s_02_1_e1`), and every committed record and fixture under `hyalos-os/`, which still audits.
- The KVM backend runs any known profile with the current profile's settings, because the kernel image is built from those settings. A profile with other settings is refused there, as before.
- Replay still compares every byte. Had `2.0.3` counted fuel differently, an honest profile 1 record would be refused, never passed.

## Consequences

- A new run's task identity changes, because the version 4 identity includes the profile's name. A customer authorises the profile 2 identity for new runs. An authorisation of a profile 1 identity covers only runs that name profile 1.
- A new run's transcript differs from a profile 1 run only in `profile` and `task`. `core_s_02_1_e1` checks this for both seeds on both backends.
- `hyalos-verify` reads both profiles, and `--version` prints `profiles hyalos.profile/1 hyalos.profile/2`. Its binary changes because `wasmi` is compiled into it. Publishing a verifier release is the operator's decision (ADR-0131).
- Adding a pair to `CHECKED_EQUAL` is a decision of this kind and needs its own ADR. A `wasmi` update that changes fuel or a transcript cannot be added to it, and records of the older profile would then need that older `wasmi` to replay.
- `2.0.3` adds `unsafe` of its own: `wasmi::Error` holds a `NonNull` to the static trap code and has `unsafe impl Send` and `Sync`. `cargo xtask hyalos-unsafe` checks only hyalOS's files, and ADR-0133 leaves third-party code to the review scope. The review scope in `docs/plans/hyalos-kernel-core` (#467) names `wasmi` `2.0.2`. That slice updates it to `2.0.3`, including this `unsafe`, when it resumes.
