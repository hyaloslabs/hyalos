# ADR-0004: One signed manifest per job, enforced only by the broker

**Status:** Accepted on November 1, 2026, after review against ADR-0002, 0005, 0006 and 0012. The clarifications from that review are folded in below.

## Context

Claims, virtual keys, job tokens, scoped git tokens and per-route ACLs each encoded part of a job's authority.

## Decision

The engine signs one manifest per job: repo and branch prefix, actions, egress hosts, tools, data flows, budget, deadline, the step's role, and taint. The broker enforces that document and nothing else. The signing key lives only in the engine ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)).

**Taint is derived, not requested.** The engine computes it from the step's inputs. A step whose actor reads forge-authored content gets a tainted manifest, and the engine cannot sign an untainted one for it ([ADR-0005](0005-provenance-is-a-type.md)). A tainted manifest cannot push outside its branch prefix, merge, or use a subscription token, and the broker content-gates its outbound text.

**The role is in the manifest.** The broker learns whether a step is a reader, planner, actor or deterministic step from the manifest, so a reader's key can refuse tools at runtime ([ADR-0005](0005-provenance-is-a-type.md)).

**Irreversible actions carry their approval.** The broker enforces only the manifest, so a human approval ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)) reaches the broker as a signed reference inside it. A merge to a protected branch is allowed only with an approval bound to the PR's head commit SHA; without one the broker refuses.

**Plugins narrow a job, never widen it.** Where a plugin runs in a job, the effective permissions are the intersection of the plugin's granted capabilities and the job's manifest ([ADR-0005](0005-provenance-is-a-type.md), [ADR-0006](0006-plugins-are-skills-and-wasm-components.md)). The manifest and plugin capabilities share one versioned vocabulary.

**Forge tokens are minted by the engine.** The engine holds the GitHub App private key (or the GitLab credential), the long-lived secret, and mints a per-job token scoped to the repo and permissions in the manifest (GitHub App installation tokens, GitLab project tokens). It hands the token to the broker with the manifest, and the token expires with the job. No operator PAT exists.

**Manifests cannot be replayed.** A manifest carries a schema version, a job id and a deadline. The broker refuses an unknown version, an expired manifest, a job id that is not a live job, or an invalid signature.

## Consequences

Authority is readable in one place and diffable between jobs. The manifest schema is the most important interface in the system: it changes only by a versioned schema change, and a field that widens authority needs an ADR. A broker compromise exposes the tokens of live jobs, not the App key, because the broker never holds it.
