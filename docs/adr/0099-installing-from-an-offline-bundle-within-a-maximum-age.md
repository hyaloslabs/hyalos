# ADR-0099: Installing from an offline bundle within a maximum age

**Status:** Proposed

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) has an air-gapped host install from an offline install bundle. The bundle holds the plugin, its Sigstore bundle and a revocation list as of the day it was built, all signed by the pipeline's identity. ADR-0007 also caps ages: the trusted-root snapshot may be at most 90 days old and the revocation list at most 30. Age is the host clock minus the signed time inside each. Past a limit, new installs and updates are refused while installed plugins keep running. The limits are part of the install policy, so changing them needs a human.

Most of that already exists:
- [ADR-0090](0090-a-plugin-index-served-as-release-assets.md) lets `install --index` take a directory holding a release's files.
- [ADR-0097](0097-the-trusted-root-snapshot-the-pipeline-signs-and-importing-a-newer-one.md) and [ADR-0098](0098-revoking-a-proven-malicious-release.md) put the signed snapshot and the signed revocation list in every release, import them from a location, and record each import as an event.
- [ADR-0091](0091-the-install-policy-a-capability-ceiling-the-operator-signs.md) put `max_snapshot_age_days` and `max_revocation_age_days` in the install policy, which only a human sets.

Nothing enforces the ages yet. S7.6 (#47) asks for that, and for a test that a bundle verifies with no network exactly as it does online.

## Decision

**The offline bundle is a release directory.** A release of `hyaloslabs/marketplace`, downloaded as it is published, is the offline install bundle: `index.json`, each `.skill` archive and the three signed files beside it. These are the archive's Sigstore bundle, `trusted-root.json` and `revocations.json`. `hyalos plugin install --index <dir> <name>` reads it with no network. The check is the same as online: the index digest, the archive's Sigstore bundle under the snapshot in force, the snapshot import and the revocation-list import. Nothing is fetched for any of it, since the Sigstore bundles carry their inclusion proofs and the snapshot carries the trusted root. No new format or command is added.

**The maximum ages.** Before `install --index` reads an archive, and before `update` checks a plugin, the snapshot in force and the revocation list in force are checked against the host clock, after whatever the location brought has been imported. The limits come from the install policy (`max_snapshot_age_days`, `max_revocation_age_days`), or default to 90 and 30 days when no policy is set. Each age is measured from the earliest signed time the item's own Sigstore bundle proves, recorded when it was imported, or embedded for the snapshot.

- A snapshot older than its limit refuses with `snapshot.too_old`.
- A list older than its limit refuses with `revocations.too_old`.
- No list in force at all refuses with `revocations.absent`. Without a list, what is revoked cannot be told, so this fails closed (ADR-0009). Every release has carried one since r10.
- Each message says to import a newer release with `hyalos plugin snapshot import <location>`, or to install from one.
- `update` reports the plugin as not updated and exits 1. The installed version keeps running.
- A signed time after the host clock does not count as old. Accepting a bundle already rejects a signed time in the future.

Nothing that is already installed is touched. A job is still given a plugin whose snapshot or list has gone stale. Only revocation (ADR-0098) disables one.

**What the ages do not cover.** A local install from a directory (`hyalos plugin install <dir>`) checks no signature, so it has no age to check. It still refuses a digest the revocation list in force revokes, and refuses everything if that list cannot be read (ADR-0098).

**Changing a limit** is `hyalos plugin policy set --max-snapshot-age-days <n> --max-revocation-age-days <n>`, between 1 and 3650 days. It is a human decision signed with the operator's key, like the rest of the install policy (ADR-0091, ADR-0012). No MCP tool makes it.

**Tests.**
- `core/hyalos/tests/fixtures/marketplace-r10` holds release r10's files exactly as published. `core/hyalos/tests/offline.rs` installs from them with the binary itself, under the snapshot it embeds and the pipeline's real signatures, with no test trust. The operator's policy widens the ages so the fixture does not expire.
- The same install then runs again in an `ubuntu:24.04` container with `--network none`, and the test checks that it records the same evidence (the snapshot, list and archive digests) as the install on the host. It skips without a container runtime, unless `HYALOS_REQUIRE_CONTAINER` is set, as CI sets it.
- `core/hyalos/tests/plugin.rs` shows that a snapshot past its age refuses both install and update while the installed version stays, and that only a signed policy change lifts the refusal.

No event, wire message, manifest or state image changes.

## Consequences

- An air-gapped host must bring in a release at least every 30 days by default, or every period its policy sets. Until it does, it can install nothing new. Weekly releases (ADR-0097) leave room to spare.
- A host with a wrong clock may refuse installs it should allow, or allow installs a little past their age. ADR-0007 accepts this: anyone who controls the clock already controls the install.
- An index without a revocation list, such as one a third party serves or a release before r10, can no longer be installed from on a host that never imported a list. This fails closed, and the error says so.
- The fixture pins a real pipeline release. The test does not depend on whether it brings a newer snapshot than the one embedded. If the embedded snapshot can no longer verify it after a Sigstore rotation, the test fails, and that is the signal to refresh the fixture.
