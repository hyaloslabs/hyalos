# ADR-002: Keep the pinned wasmi, and make each record name its execution profile

**Status:** accepted (developer, 2026-11-22); amended by hyalos-kernel-verifier ADR-007, which moves ownership of the profile to that pack

**Proposed by:** author (parent-level pass); accepted by the developer

## Context

The parent pack left open which interpreter to use and what sits under the kernel. The spike uses `wasmi` `=2.0.2`, with no JIT.

Replay compares the whole transcript byte for byte, and the transcript includes `fuel_used` (`spikes/task-kernel/kernel/src/main.rs:402`). Fuel is counted by the interpreter, so the interpreter's version and settings are part of what a record means. The record does not name them (`spikes/task-kernel/RECORD.md`). An interpreter update that changes fuel accounting would make honest old records fail replay, and nothing in the record would say why.

The same `wasmi` settings are also written out twice, in `kernel/src/main.rs` and `hyalos/src/main.rs`. Records stay identical across the two backends only while those copies agree.

`wasmi` 2.0.2 and its helper crates are about 41,900 non-blank, non-comment lines, and `wasmparser` 0.228.0 about 27,600. That is more than twice the trusted core's whole budget, in code we did not write. Its audit status is not established (#412).

## Options

1. **Keep `wasmi`, pinned.** Chosen. It works, it has no JIT, and it passed the spike's hostile-guest tests.
2. **Change to another interpreter now.** Not chosen: no alternative has been evaluated, and nothing measured so far shows `wasmi` is the wrong choice.
3. **Write a small interpreter for the subset guests use.** Deferred: it would shrink what must be reviewed, but replay would then rest on new code of ours. Reconsider if #412 finds `wasmi` too costly to review.
4. **Put seL4 under the kernel.** Deferred, not evaluated. It would strengthen isolation under the kernel. It would not remove the interpreter from the trusted path, so it does not settle this question.

## Decision

- Keep `wasmi` at an exact pin, with no JIT.
- Define one **execution profile**: the interpreter and version, its settings, the fuel limit and the memory limit. Both backends and the verifier build from that one definition.
- The next record version names its profile. A verifier refuses a profile it does not have.
- Changing anything in a profile creates a new profile. A verifier keeps the old ones, so old records still replay.
- Floats stay refused. A profile that allows them must build `wasmi` with its `deterministic` feature, which canonicalises NaN results. The spike does not enable that feature today.
- A language runtime such as Python is guest code, never part of the kernel, the monitor or the profile. Its bytes are pinned by the guest hash.

## Consequences

- `hyalos-kernel-core` owns the profile, and the verifier consumes it.
- Each profile a verifier supports adds interpreter code to it, so profiles should change rarely.
- A security fix in `wasmi` needs a new profile, and a decision on whether records from the old one are still accepted.
- This is a decision not to switch. It is not a comparison of interpreters.
