# ADR-0135: hyalOS's review scope follows its lockfiles, and its upstreams' advisories fail a daily check

**Status:** Accepted

## Context

hyalOS's security claim rests mostly on code it did not write: `wasmi`, the KVM crates, `seccompiler` and `landlock`, TLSNotary at `v0.1.0-beta.1`, and the signature, hash and TLS crates under the audit and the attestation tool (#412). An independent review needs to know which of these it covers. The plan pack [`hyalos-kernel-core`](../plans/hyalos-kernel-core/README.md) chose the positions in its [ADR-005](../plans/hyalos-kernel-core/decisions/ADR-005-review-scope-and-the-tlsnotary-pin.md) and [ADR-008](../plans/hyalos-kernel-core/decisions/ADR-008-the-review-covers-our-use-of-tls.md): our use of each is covered, `wasmi`'s internals are covered, and the internals of TLSNotary and of the cryptographic and TLS crates are excluded.

A written scope goes stale when a pin moves. And the checks CI had read only RustSec, through `cargo deny`, on pull requests that touch `hyalos-os/`: an advisory published against a pin that has not moved waited for the next such pull request, and a TLSNotary advisory, a git dependency with no crates.io entry, might never reach RustSec. GHSA-qrr7-2g7f-r9rg showed both on 2026-11-24: GitHub published it against `wasmi` 2.0.2 before RustSec listed it ([ADR-0134](0134-hyalos-profile-2-moves-to-wasmi-2-0-3.md)).

This is the repository's record of the pack's Slice 11.

## Decision

**The review scope is a file the lockfiles hold to, and an advisory against a pinned version fails a daily job.**

- **The scope.** `hyalos-os/review/scope.toml` has one row per component and part (`use` or `internals`), each with its version, its upstream audit (a citation, or `none found` with the date searched), `covered` or `excluded`, and a reason. Every `upstream_audit` says `searched <date>`, except that an `excluded` utility crate may say exactly `not searched (utility, <date>)`, since writing `none found` for a search that was not made would be false; a `covered` row may not. TLSNotary's rows give the tag and the locked commit. Transitive crates the claim rests on have rows of their own, so they follow the lockfiles too: `wasmparser`, which parses and validates every guest for `wasmi`, and the trust anchors and chain check, `webpki-roots` and `rustls-webpki`.
- **`cargo xtask review-scope`** reads the scope and hyalOS's three lockfiles. It fails when a row's version, or TLSNotary's tag or commit, is not what a lockfile pins; when a direct dependency of a hyalOS crate (dev and build dependencies included) has no row; when a listed crate is locked at a version no row names; when a field is missing or empty, a `part` or `position` value is unknown, or an `upstream_audit` has no search date or spells `not searched` another way; when a component ADR-005 or ADR-008 splits lacks its `use` or its `internals` row, the internals row's position differs from theirs, or a `use` row they cover (the split components, `kvm-ioctls`, `kvm-bindings`, `seccompiler`, `landlock`) is missing or not `covered`; and on any line it cannot read, an unknown lockfile table, or a lockfile with no workspace member. CI's `lint` job runs it on every pull request, beside hyalOS's other `xtask` checks, so a change to the check or to any lockfile meets it and the `hyalos-os` job's path filter is unchanged.
- **`cargo xtask advisories`** matches saved GitHub advisory feeds of `wasmi-labs/wasmi` and `tlsnotary/tlsn` against every package the three lockfiles pin, comparing a git pin's tag with its `v` dropped, by SemVer order, and a crate's name with `-` and `_` alike. It fails naming each advisory whose affected range includes a pinned version, and it fails on a missing feed, a feed that is not a list of advisories, or a range it cannot read. An advisory that names no package or range is taken to affect every version of the repository's crate. Withdrawn advisories and other ecosystems' packages are not findings.
- **RustSec on a pull request.** CI's `hyalos-os` job runs `cargo deny` on the workspace's lockfile as before, and `cargo deny check advisories` on the kernel image's and the guest's, since the kernel image locks `wasmi` too.
- **`.github/workflows/hyalos-advisories.yml`** runs daily and on manual dispatch, with read-only permissions. It first runs `cargo deny` on a canary crate it writes, pinned to a version with a known RustSec advisory, and fails unless that advisory is reported, so an advisory database that was not read cannot let the run pass. It then runs `cargo deny check advisories` on all three hyalOS lockfiles, reads both GitHub feeds through the REST API with the run's token (30-second limit, one retry), fails unless the `wasmi` feed holds GHSA-qrr7-2g7f-r9rg (published, and not affecting the pin), so an empty answer cannot pass for a read one, runs `cargo xtask advisories` on them, and runs `cargo xtask review-scope`.
- **Exceptions.** A RustSec exception stays as it is: an `ignore` entry in `hyalos-os/deny.toml` with a reason, and an assessment under `hyalos-os/advisories/`. The GitHub feed check has no exception list. An advisory it finds is taken to the developer, who moves the pin or decides how that advisory is excepted.
- **Leaving TLSNotary's beta pin** follows `hyalos-os/review/pin-policy.md`, which writes out the pack's ADR-005.

## Consequences

- A pin that moves fails CI until its row is read again, and a new direct dependency fails until it has a position.
- An upstream advisory against a pin fails a run within a day, whether or not anyone touches hyalOS.
- The scope's audit citations are a web search's result on the date each row gives. A later audit changes a row without a new decision.
- `wasmi`'s new upstream `unsafe` in 2.0.3 is reviewed through its `internals` row, since `cargo xtask hyalos-unsafe` reads only hyalOS's own code.
- The daily job reads only the two GitHub repositories the design names. Advisories GitHub publishes against other crates reach us through RustSec.
- The check does not parse TOML in general: the scope's lines are a restricted form, and a lockfile is read for its packages only. A form either cannot read fails it.
