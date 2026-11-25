# hyalOS guest interface

The guest interface is versioned. This file states every version a backend implements, and the
current one is the name the monitor holds as `hyalos_audit::ABI` and the kernel image as `ABI`.
A record does not carry this name (ADR-004 of the `hyalos-kernel-core` plan pack); the interpreter
settings a record names are its execution profile (`hyalos.profile/1` or `hyalos.profile/2`,
`RECORD.md`), which this file does not restate.

Both backends, the ordinary process and the kernel image on KVM, are held to this file by the
conformance suite in `monitor/src/abi_tests.rs`. A change to a call's name, signature, limit,
result or refusal fails that suite until it is given a new interface name, a new section here and
an ADR. A section here is never edited once a backend implements it: the suite pins the digest of
the whole section.

## `hyalos.abi/1`

A guest is a `wasm32-unknown-unknown` module. It must export a function `run` with no parameters
and no result, and its linear memory as `memory`. It has no start function.

Its only imports are these four functions, from the module `hyalos`. All values are `i32`.

| Call | Signature | Limit | Results |
|---|---|---|---|
| `read` | `(index, ptr, cap) -> len` | Record length | Copies input record `index` to `ptr` and returns its length. `-1` no such record, `-2` `cap` is smaller than the record |
| `infer` | `(schema, ptr, len) -> label` | 4,096 bytes | Asks the approved model the question at `ptr`. Schema `0` classifies an issue and returns `0` bug, `1` feature, `2` security or `3` other |
| `effect` | `(kind, ptr, len) -> 0` | 65,536 bytes, one per run | Kind `0` sends one notification with the body at `ptr` and returns `0` |
| `return` | `(ptr, len)` | 4,096 bytes | Ends the run with these bytes as its result. It never returns to the guest |

A refusal is not a return value: it ends the run with the reason below, and no later call is
made. A call's argument checks (`UnknownSchema` or `UnknownEffect`, `EffectLimit`, `TooLarge`,
`BadPointer`) run in the order listed, so the first that fails is the reason. The reasons after
them come from a live run or a replay once those checks pass.

| Where | Reason | When |
|---|---|---|
| loading | `InvalidModule` | The bytes are not a module the profile accepts, for example one using floats, SIMD, atomics or a start function |
| loading | `BadImport` | An import other than the four calls, or one of them with another type |
| loading | `BadExport` | No `run` function with no parameters and no result, or no `memory` export |
| `read` | `BadPointer` | The record does not fit in the guest's memory at `ptr` |
| `infer` | `UnknownSchema` | `schema` is not `0` |
| `infer` | `TooLarge` | `len` is over 4,096 |
| `infer` | `BadPointer` | The question is not inside the guest's memory |
| `infer` | `ModelUnavailable` | A live run's attestation tool could not be run or finish, or failed in a way it did not name |
| `infer` | `ModelRefused` | A live run's question is not UTF-8, or the answer is not a label or not attested by the presentation it names |
| `infer` | `NotaryUnreachable` | A live run has no notary, or cannot reach it |
| `infer` | `NotaryDeadline` | The notary did not answer in time |
| `infer` | `WrongNotaryKey` | The notary signed with a key the operator did not trust |
| `infer` | `ReleaseMismatch` | The notary is not the pinned release |
| `infer` | `ProviderRefused` | The model's provider refused the request |
| `infer` | `NoAuthorisation` | The notary holds no authorisation for this run |
| `infer` | `AttemptsSpent` | The authorisation's attempts are spent |
| `infer` | `CanaryHeld` | The notary's canary holds new sessions |
| `infer` | `LedgerFailed` | The run's signed start time cannot be read or waited for |
| `infer` | `ReplayDiverged` | A replay asked a question the record does not hold |
| `effect` | `UnknownEffect` | `kind` is not `0` |
| `effect` | `EffectLimit` | The run already performed its one effect |
| `effect` | `TooLarge` | `len` is over 65,536 |
| `effect` | `BadPointer` | The body is not inside the guest's memory |
| `effect` | `EffectFailed` | A live run could not send the notification |
| `effect` | `ReplayDiverged` | A replay sent a body the record does not hold |
| `return` | `TooLarge` | `len` is over 4,096 |
| `return` | `BadPointer` | The result is not inside the guest's memory |
| running | `OutOfFuel` | The guest used all its fuel |
| running | `GuestTrap` | The guest trapped, for example by growing its memory past the profile's limit or recursing without bound |
| running | `NoResult` | `run` returned without calling `return` |

The guest has no files, network, processes, threads, clock, randomness or floating point. Its
memory and fuel limits are the profile's.

Each input record is one line of UTF-8, with no newline: `<issue number>\t<issue title>`. Treat
titles as untrusted.

## Not part of the interface

These reasons come from a backend or a run's setting rather than from a call, so no version of the
interface names them.

| Reason | When |
|---|---|
| `KernelFault` | The kernel image faulted, halted or panicked, or the machine could not be built, or the machine runner died, closed its channel or broke `hyalos.runner/1` |
| `BadHypercall` | The kernel image used a port, request or exit the monitor does not answer |
| `Timeout` | The KVM backend's deadline passed. The process backend has no deadline |
| `NoKvm` | The KVM backend could not open or set up KVM |
| `TooLarge` | The KVM backend refuses a guest over 1 MiB before it loads. The process backend has no such limit |
| `GuestMismatch` | A replay was given a guest whose hash is not the record's |
| `ReplayDiverged` | A replay's run differs from the record, or names a profile this backend cannot run |
| `NotaryUnreachable` | A live run with no notary is refused before the guest starts |
