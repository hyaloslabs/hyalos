# ADR-0091: The install policy, a capability ceiling the operator signs

**Status:** Accepted

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) has operators set install policy once, so updates within it install automatically and one that adds a capability beyond it waits for a human. A verifier agent can never grant one ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)). The same ADR puts the maximum ages of the trusted-root snapshot and the revocation list (90 and 30 days) in install policy, where changing them is a policy change and needs a human.

S7.5 (#46) asks for this policy. [ADR-0089](0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md) made every install a human's decision. [ADR-0090](0090-a-plugin-index-served-as-release-assets.md) added installing by name from an index, and said install policy must not install from an index on its own authority before Sigstore bundles are verified (S7.12).

Two shapes were possible. One is the Cedar policy, with a new action per install. The other is a decision of its own with a fixed shape. A Cedar rule could express more, but the question is narrow: which capabilities, from which sources. A fixed shape can be shown as a capability diff, checked in the state on replay, and proved about without a solver.

## Decision

**The policy is its own decision.** `plugin.policy_set { policy, by, reason, signed }` records an `InstallPolicy`:

- `ceiling`: a capability set in the ADR-0085 vocabulary.
- `indexes`: the index locations it trusts, at most 16.
- `max_snapshot_age_days` and `max_revocation_age_days`: each 1 to 3,650, defaulting to 90 and 30.

Like every decision ([ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md), [ADR-0070](0070-the-operators-signature-on-every-human-decision.md)), it is written only by `Engine::set_install_policy` and only by a human, since it decides what installs without one. It is signed with the operator's key, and refused raw from `submit` and from the broker. A later one replaces it. `hyalos plugin policy set --ceiling <file> [--index <location>]...` prints the ceiling as a diff against the policy in force, then records it after the passphrase. It joins `tools_cmd::DECISIONS`. `hyalos plugin policy show` prints it.

**Policy installs only what the policy covers.** `plugin.installed` by `policy` is accepted, on write and on replay, only when all of these hold:

- A policy is in force.
- The install's source is an index the policy lists.
- The grant asks for nothing beyond the ceiling.

A local install never goes by policy. A verifier never installs, whatever the policy says. A human may install anything, as before. `hyalos plugin install --index <location> <name>` takes the policy's route by itself when it applies: no passphrase, and the event is recorded by `policy` and unsigned. Otherwise it is the operator's decision, with `--grant` for anything the diff adds. An install by policy stays valid if the policy changes later; the state checks only that it was by human or policy and not local.

**Listing the index is the trust, until bundles are verified.** Before S7.12, an index location is not authenticated. The operator's listing of it is what lets policy act at all, and the ceiling bounds what any plugin from it can be granted. An index the policy does not list never installs without the operator. When S7.12 lands, an install by policy also needs a verified bundle.

**The ages are kept, not yet used.** They are in the policy now so that changing one is this decision. S7.6 and S7.11 check them when snapshots and revocation lists exist.

This is schema version 17 (`plugin.policy_set`, no payload changed) and `StateImage` version 12 (`install_policy`).

## Consequences

- Within the policy, installs and updates (S7.10) need no human. Anything beyond it still does, shown as a diff.
- The policy is one value in the state, so the engine checks a policy install on replay with no solver and no Cedar evaluation.
- A policy with no index lists nothing to trust, and installs nothing by itself.
- Until S7.12, a listed index that is compromised can install, without the operator, any plugin whose capabilities are within the ceiling. That is the risk the ceiling exists to bound, and it ends when bundles are verified.
