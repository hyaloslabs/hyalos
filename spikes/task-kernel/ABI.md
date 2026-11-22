# hyalOS kernel v0: guest interface

A guest is a `wasm32-unknown-unknown` module. It must export a function `run` with no parameters and no result, and its linear memory as `memory`. It has no start function.

Its only imports are these four, from the module `hyalos`. All values are `i32`.

| Call | Signature | Meaning |
|---|---|---|
| `read` | `(index, ptr, cap) -> len` | Copy input record `index` to `ptr`. Returns its length, `-1` if no such record, `-2` if `cap` is too small. |
| `infer` | `(schema, ptr, len) -> label` | Ask the approved model the question at `ptr`, at most 4,096 bytes. Schema `0` classifies an issue: `0` bug, `1` feature, `2` security, `3` other. |
| `effect` | `(kind, ptr, len) -> 0` | Perform an action. Kind `0` sends one notification with the body at `ptr`, at most 65,536 bytes. A guest may perform **one** effect per run. |
| `return` | `(ptr, len)` | End the run with this result, at most 4,096 bytes. It never returns to the guest. A run that ends without calling it fails. |

The guest has no files, network, processes, threads, clock, randomness or floating point. A module that uses floats, SIMD, atomics or any other import is refused before it runs. Memory is limited to 4 MiB and execution to a fixed amount of fuel. Any refusal ends the run.

Each input record is one line of UTF-8, with no newline: `<issue number>\t<issue title>`. Treat titles as untrusted.
