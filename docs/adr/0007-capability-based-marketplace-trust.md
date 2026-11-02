# ADR-0007: Marketplace trust is capability-based and automated

**Status:** Proposed. Depends on [open question 3](../planning/open-questions.md) (keyless signing and air gaps).

## Context

Identity-based trust (publisher keys, a human approving each signing, revocation) asks whether the publisher can be trusted, and the Cursor incident showed that answer can be wrong. The per-release human gate does not scale and is the part of today's design we like least.

## Decision

Trust what a plugin can do, not who published it. Every release builds reproducibly in CI, is signed keylessly against the CI workflow's identity, is recorded in a public transparency log, and passes automated evidence: static analysis, sandboxed review agents, the red-team corpus and a capability diff. Publication is decided by policy: same or fewer capabilities publish automatically, new capabilities publish flagged for consent, failed checks block. Operators set install policy once; updates within policy install automatically, updates that add a capability wait for consent. Air-gapped installs use an offline signed bundle.

## Consequences

No per-release human signer. The security of a plugin rests on the runtime's enforcement of capabilities, so ADR-0006's capability surface must stay small and well tested. Revocation remains for proven-malicious releases.
