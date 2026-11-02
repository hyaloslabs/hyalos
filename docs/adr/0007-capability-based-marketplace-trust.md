# ADR-0007: Marketplace trust is capability-based and automated

**Status:** Accepted on November 1, 2026, after review against ADR-0003, 0006 and 0012 and after spike #73 verified Sigstore bundles offline with sigstore-python 4.5.1 and sigstore-rust 0.15.1. The clarifications from that review are folded in below.

## Context

Identity-based trust (publisher keys, a human approving each signing, revocation) asks whether the publisher can be trusted, and the Cursor incident showed that answer can be wrong. The per-release human gate does not scale and is the part of today's design we like least.

## Decision

Trust what a plugin can do, not who published it. Every release builds reproducibly in CI, is signed keylessly, is recorded in a public transparency log, and passes automated evidence: static analysis, sandboxed review agents, the red-team corpus and a capability diff. Publication is decided by policy: same or fewer capabilities publish automatically, new capabilities publish flagged for consent, failed checks block.

**Whose identity is pinned.** The signature is made by the Hyalos marketplace pipeline's release workflow, so that is the only identity a verifier accepts: the certificate's subject, its issuer and the workflow's ref, built into Hyalos and changeable only by a Hyalos release. It is not the plugin author's identity, because an author's own CI says nothing about whether the pipeline's checks ran. A signature from any other identity is refused.

Operators set install policy once; updates within policy install automatically. An update that adds a capability beyond that policy waits for **human** consent. A verifier agent can never grant it ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). Plugins that did not go through this pipeline install in local mode ([ADR-0006](0006-plugins-are-skills-and-wasm-components.md)), where every capability needs a human.

**How a bundle is verified.** Every install, online or offline, runs the same check on a Sigstore bundle and refuses on any failure:

- The signature, the pinned identity and the artifact's digest match.
- The bundle holds a transparency-log entry with its inclusion proof and signed checkpoint. A release with no log entry is refused, with or without a network.
- The bundle holds at least one verified signed time (the log's or a timestamp authority's). The certificate lives only minutes, so it is checked against that signed time, never against the local clock; a release from a year ago still verifies.
- Everything chains to the trusted-root snapshot. An authority the snapshot does not list refuses the bundle; there is no weaker fallback.

The check is one pinned library in the trusted core, sigstore-rust at an exact version. CI runs it against the spike's vectors and the library's own conformance tests.

**Air-gapped installs** use an offline install bundle. It holds the plugin, its Sigstore bundle and a revocation list as of the day it was built, all signed by the same pipeline identity. The verification above needs no network, because the bundle carries the inclusion proof and the snapshot carries the trusted root. No key-based path is needed.

**The trusted-root snapshot** is Sigstore's trusted root, republished by the Hyalos pipeline after it fetches it through Sigstore's own update protocol (TUF), and signed by the pipeline identity so that it carries a signed issue time. Sigstore's TUF metadata cannot travel into an air gap because its freshness record expires in about a week. Each Hyalos release embeds the current snapshot. A newer one is accepted only if it verifies under the snapshot already installed and is newer than it; an older one is never accepted. If Sigstore rotates so that the installed snapshot cannot verify its successor, the install refuses it and needs a new Hyalos release.

**Maximum age.** The snapshot is at most **90 days** old and the install bundle's revocation list at most **30 days**. Age is the host clock minus the signed time inside the artifact. The host clock is trusted only for this staleness guard, since anyone who controls it already controls the install. Past a limit, the install refuses new installs and updates until a newer snapshot or bundle is imported. Installed plugins keep running. These are defaults; the operator sets the limits in install policy, and changing them is a policy change, which always needs a human ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). Importing a snapshot or bundle is recorded as an event ([ADR-0003](0003-event-log-is-the-database.md)).

## Consequences

No per-release human signer. The security of a plugin rests on the runtime's enforcement of capabilities, so ADR-0006's capability surface must stay small and well tested. The pipeline's release workflow becomes the one trust anchor for plugin signatures, snapshots and revocation lists, so its identity, its protected branches and its tags need the strongest protection we have.

Revocation remains for proven-malicious releases, but it cannot reach an air-gapped install until that install updates; the 30-day limit bounds that window. The 90-day snapshot limit bounds how long an authority that Sigstore has removed stays trusted offline. Sigstore's trusted root changed three times in the 16 months before this decision, with gaps of 81 and 220 days, so a snapshot at the limit is at most one change behind. When it is behind, it fails closed: it refuses bundles that use a newer authority, and installs stop until it is updated. It never accepts on a weaker check.

The spike's verification took about 3 ms and ran with no network, so offline installs cost nothing extra. Not yet shown, and covered by the stories that build this: a bundle logged in Sigstore's new Rekor v2 (the current snapshot already lists that log), a bundle signed by a GitHub Actions workflow identity, and the verifier running inside the Hyalos binary on every supported platform. The library is a year old and pre-1.0, so its version is pinned and a change to it re-runs the vectors.

## References (read November 2, 2026)

- Spike #73 findings: [docs/planning/spike-73-findings.md](../planning/spike-73-findings.md). The evidence for the verification rules, the two verifiers' results and the snapshot dates above. The rig and test vectors are in `spikes/73/`.
- Sigstore bundle format: [sigstore_bundle.proto](https://github.com/sigstore/protobuf-specs/blob/main/protos/sigstore_bundle.proto).
- The Rust verifier: [sigstore/sigstore-rust](https://github.com/sigstore/sigstore-rust).
