# ADR-004: The SDK is generated Python source over a native `_hyalos` module

**Status:** accepted (developer, 2026-11-25); amended by ADR-005, the native call `return` is named `finish`

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

The script must see only the calls its contract grants, with no client, key, model setting, URL or
tool list (parent brief). The parent expects each runtime to expose the four calls its own way
(Monty through host functions, MicroPython through a native module) while the Python surface stays
the same. The SDK's bytes must be deterministic and bound into the task identity (S-02).

## Options

1. **Generate the SDK inside each runtime's native code.** Rejected: two generators, the SDK's
   bytes would be part of the interpreter, and the same script could not be shown to run on the
   same SDK.
2. **Generate one Python source module from the contract, delivered as input record 1, over a
   minimal native module `_hyalos` that each runtime provides with exactly the four calls.**
   Chosen.
3. **Write the generator in Rust inside `hyalos-os/`.** Rejected: it is not monitor code, it would
   spend budget, and capsules owns the real generator.

## Decision

A generator under `spikes/438/` turns a contract into one Python source file with a fixed layout:
`task.read_inputs()`, `task.ask.<name>()` per granted schema, `task.notify.<name>()` per granted
effect, and `task.finish()`. It uses only a fixed name table (schema `0` is `classify`, effect `0`
is `team`), sorts nothing at run time and accepts only the canonical byte form of a contract. Each
runtime guest provides `_hyalos` with `read`, `infer`, `effect` and `return`, and the guest's `run`
loads record 1 as the `hyalos` module and executes record 2.

## Consequences

- One SDK file is shared by both runtimes, so S-04 and S-05 compare the runtimes, not two SDKs.
- `_hyalos` is reachable from a script, so the monitor's check is the boundary (S-04.3, S-05.2), as
  the parent brief requires.
- The generator's language is the spike's choice (Python run on the host is the default); it is
  not part of any guest.
