# ADR-012: `hyalos.abi/2` is written whole and pinned once

**Status:** accepted (developer, 2026-11-25); amended by ADR-016, the pin waits for the ceiling ADR; amended by ADR-019, the #411 measurement may run version 5 before the pin; amended by ADR-023 and ADR-025, a pre-pin default build reads version 5 as inconclusive

**Proposed by:** author (spec); accepted by the developer at the spec design checkpoint

## Context

`hyalos-os/ABI.md:9-13` forbids editing a section once a backend implements it, and the abi/2 rules (framing, the contract, closed types, the limit) land over several slices.

## Options

1. **Pin the section in the framing slice and edit it later.** Rejected: breaks the repository's rule.
2. **All abi/2 behaviour in one slice.** Rejected: far larger than one PR.
3. **Write the section whole in the framing slice under a draft line, unpinned; only tests write version 5; the slice completing the last abi/2 rule removes the line, pins the digest and moves `CURRENT_VERSION` to 5 in one commit.** Chosen.

## Decision

The abi/2 section is committed whole, marked `(draft: not implemented)` and without a pinned digest, in the slice that frames inputs. While the line stands, `run` and `live` write version 4. The slice that completes the last abi/2 rule removes the line, pins the digest in `abi_tests.rs` and sets `CURRENT_VERSION` to 5, in one commit. After that the section is never edited.

## Consequences

No default command writes version 5 until the interface is complete. A slice that needs to change the draft section may, until it is pinned.
