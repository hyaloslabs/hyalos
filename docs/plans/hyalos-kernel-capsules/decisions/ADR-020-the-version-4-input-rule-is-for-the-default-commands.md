# ADR-020: The version 4 input rule applies to the default commands

**Status:** accepted (developer, 2026-11-25)

**Proposed by:** author (slice); accepted by the developer at the slice checkpoint

## Context

This amends ADR-018. The spike's `spike-run` (`hyalos-os/monitor/src/spike.rs:84`) runs the committed `micropython-hostile-utf8` case with a non-UTF-8 record, which S-01.4 keeps, until the spike rig is removed (ADR-011).

## Options

1. **Every version 4 run refuses such records.** Rejected: it breaks a committed spike case S-01.4 must keep.
2. **The rule applies to the version 4 runs of `run`, `live`, `fuzz`, `task`, `attacks` and `bench`; `spike-run`, in a spike build only, keeps its own line split until it is removed.** Chosen.

## Decision

The default commands' version 4 runs refuse `BadInputs` on a record holding a newline, empty, or not UTF-8. `spike-run` is exempt while it exists, and only in a spike build.

## Consequences

No default build has a line-form reader. The exemption ends with the spike rig.
