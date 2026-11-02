# ADR-0007: Marketplace trust is capability-based and automated

**Status:** Proposed. The original open question on keyless signing and air gaps was settled in the November 1, 2026 planning session; folded in below. The offline verification still needs an E7 spike to confirm.

## Context

Identity-based trust (publisher keys, a human approving each signing, revocation) asks whether the publisher can be trusted, and the Cursor incident showed that answer can be wrong. The per-release human gate does not scale and is the part of today's design we like least.

## Decision

Trust what a plugin can do, not who published it. Every release builds reproducibly in CI, is signed keylessly against the CI workflow's identity, is recorded in a public transparency log, and passes automated evidence: static analysis, sandboxed review agents, the red-team corpus and a capability diff. Publication is decided by policy: same or fewer capabilities publish automatically, new capabilities publish flagged for consent, failed checks block.

Operators set install policy once; updates within policy install automatically. An update that adds a capability beyond that policy waits for **human** consent. A verifier agent can never grant it ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)).

**Air-gapped installs** use an offline signed bundle. A Sigstore bundle carries the certificate and the transparency-log inclusion proof, so it verifies offline against a shipped trusted-root snapshot. No key-based path is needed. The bundle and the trusted-root snapshot each carry a maximum age; an install past the limit refuses new installs until it is updated.

## Consequences

No per-release human signer. The security of a plugin rests on the runtime's enforcement of capabilities, so ADR-0006's capability surface must stay small and well tested. Revocation remains for proven-malicious releases, but it cannot reach an air-gapped install until that install updates; the maximum age bounds how long that window can be. The offline path depends on Sigstore's bundle format, to be confirmed by a spike in E7.
