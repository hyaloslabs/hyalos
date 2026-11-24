# ADR-005: The review scope and the TLSNotary pin

**Status:** accepted (developer, 2026-11-23); amended by ADR-008, adds the TLS and certificate crates

**Proposed by:** author (spec); accepted by the developer at the spec ADR checkpoint

## Context

Issue #412 asks that the review name every third-party component the security claim depends on, and for a policy for leaving TLSNotary's beta pin. `wasmi` has public audits of `0.31.0` and `0.36.0`–`0.38.0`, and none found for the pinned `2.0.2`. One search found no published code audit of `tlsn` at `v0.1.0-beta.1` (`context.md` §2). An interpreter bug is the route to a taken-over kernel, so `wasmi` sits directly on the containment path. TLSNotary's protocol and cryptography are a specialist review of their own.

## Options

1. **Review everything, including TLSNotary's cryptography.** Rejected: it multiplies the review's cost and specialism, and would delay any review of the kernel and monitor.
2. **Review only our own code.** Rejected: most of the code the claim rests on is not ours, and the review would say little about it.
3. **Cover `wasmi` and our use of KVM and TLSNotary. Exclude TLSNotary's protocol and cryptography internals, and state that exclusion in the review's scope.** Chosen.

## Decision

| Component | Position | Reason |
|---|---|---|
| `wasmi` `=2.0.2` | Covered: the configuration and host boundary we use, and the changes since the last audited version as far as the reviewer judges useful | It is the route to a taken-over kernel, and no audit of this version was found |
| `kvm-ioctls`, `kvm-bindings` | Covered: our use | They carry the monitor's `unsafe` boundary |
| `seccompiler`, `landlock` | Covered: our use and the resulting policy | They enforce the confinement |
| TLSNotary | Our use covered: trust settings, what we accept and what we report. Protocol and cryptography **excluded** | A specialist review, and the audit report already states what an attestation does not prove |
| Signature and hash crates | Our use covered, internals excluded | Widely used primitives |

**Leaving the beta pin.** The pin moves only in a pull request of its own. It needs the forgery corpus, the presentation differential and a fresh provider canary to pass on the new version, and a changelog read that names any change to the presentation format. A format change needs a new record decision first.

## Consequences

- A review cannot be described as covering TLSNotary's cryptography.
- The scope is checked against the lockfile, so a version change forces its row to be read again.
- An upstream audit published later changes a row without needing a new decision.
