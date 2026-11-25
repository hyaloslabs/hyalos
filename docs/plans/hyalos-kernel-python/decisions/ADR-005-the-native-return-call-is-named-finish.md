# ADR-005: The native module's fourth call is named `finish`

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 1

## Context

This amends ADR-004, which says each runtime provides `_hyalos` with `read`, `infer`, `effect` and
`return`. `return` is a Python keyword, so a script or SDK cannot write `_hyalos.return(...)`, and
`design.md` §2 already calls the call `finish`. Cold-read round 1 found the two disagree.

## Options

1. **Keep `return`, reached through `getattr`.** Rejected: the SDK and every hostile script would
   need an indirection to name one call, for no gain.
2. **Name the native call `finish`; it maps one-to-one to the `hyalos` import `return`.** Chosen.

## Decision

The native `_hyalos` module has exactly `read`, `infer`, `effect` and `finish`. `finish` maps to the
`hyalos` import `return` and never returns to Python. Everything else in ADR-004 stands.

## Consequences

- `design.md` §2's `_hyalos` row stands as written.
- The guest's import list is unchanged: the four `hyalos` imports of `hyalos.abi/1`.
