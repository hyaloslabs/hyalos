# ADR-033: The triage task's input limits come from an outcome search on the shipped guest

**Status:** accepted (developer, 2026-11-27); amended by ADR-037, each search record carries `<n>\t`, so every record is asked

**Proposed by:** author (cold-read); accepted by the developer in cold-read round 5

## Context

This amends ADR-011. The plan set the triage task's `accepts` limits by reading MicroPython's heap peak from the GC allocation table and keeping it at most 1.5 MiB of the 2 MiB heap. The shipped guest, `spikes/438/guests/micropython.wasm`, has no name section, exports only `memory`, `run` and the `asyncify_*` functions, and keeps `heap` as a static in bss (`spikes/438/micropython/port/main.c:28-37`); its port turns off the `micropython` module (`port/mpconfigport.h`). Nothing locates the heap without rebuilding the guest, which would change its hash and every committed MicroPython record.

## Options

1. **Rebuild the guest with an exported heap symbol** and read the peak as planned. Rejected: a new guest hash, new records and a new review scope for one measurement.
2. **Search by outcome on the shipped guest.** Chosen.

## Decision

- With `records` fixed at 200, each record a fixed ASCII title padded with `a` to the size tried, the heap-measurement script runs the triage task on the process backend with the stub model and searches `record_bytes` for the largest size R_max at which the run ends with its one effect and no `MemoryError`. The search is not capped at `MAX_QUESTION`, since the stub model takes any question; it halves from 16,384 and then bisects to the byte.
- The margin: `record_bytes` = min(4,096, floor(0.75 × R_max)), and `bytes` = 200 × `record_bytes`. The run at those declared limits is then made on both backends and must end with its effect and byte-identical records.
- Stop if `record_bytes` would fall below 512. `records` stays 200.
- ADR 0139 records R_max, the declared limits, each command and its output. No heap peak is read.

## Consequences

The guest and its records are unchanged. The limit rests on an observed outcome, not a peak, with a quarter of the passing size held back.
