# ADR-0098: Revoking a proven-malicious release

**Status:** Accepted

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) trusts a release because the marketplace pipeline built and signed it, not because its publisher is trusted. Without a way back, a release the maintainers later prove malicious stays installable, and stays installed. ADR-0007 already names the way back: a revocation list, signed by the pipeline's identity, carried by every install bundle as of the day it was built, at most 30 days old by default (S7.6, #47).

[ADR-0097](0097-the-trusted-root-snapshot-the-pipeline-signs-and-importing-a-newer-one.md) built the trusted-root snapshot the same way. The pipeline signs it, every release carries it, and Hyalos imports a newer one from an index location only if the snapshot in force verifies it. S7.7 (#48) asks that a revoked digest refuses to install, and that installed copies are disabled with a notice.

## Decision

**The list (in `hyaloslabs/marketplace`).** `revocations.json` in the repository is

```json
{"revoked": [{"digest": "<sha256 of the .skill archive>", "reason": "malware.exfiltration"}], "version": 1}
```

A reason is a code, never free text, as Hyalos's reason codes are. Adding an entry is a pull request to `main`, so a code owner decides it. CI refuses a pull request that drops an entry (`tools/revocations.py kept`), because a revocation is permanent. The build refuses a skill whose archive is revoked, so the skill needs other bytes under a new version. `build.py` writes the list as a release carries it:

```json
{"kind": "hyalos.revocation_list", "revoked": [...], "version": 1}
```

The entries are sorted by digest, so the same entries always give the same bytes, and both builds produce it, so the two-builds comparison covers it. `release.yml` signs it with the archives and the snapshot, verifies its bundle for the pinned identity, and checks that another identity fails. A release is published when the list changed, as it is when the index did. A weekly release re-signs it even when nothing changed (ADR-0097), so the list's signed time stays within days.

**Accepting a list (`core/plugin/src/revocation.rs`).** `Revocations::verify` verifies the bundle under the snapshot in force, for `PIPELINE`, with ADR-0093's full check, before it reads anything in the file. It then requires the list's issue time to be later than the issue time of the list in force. The issue time is the earliest signed time the bundle proves. An older list is refused as `revocations.not_newer`, so no list can take a revocation back. The file must be a version-1 list of at most 1 MiB. Every entry is exactly a digest and a reason code, and no digest appears twice. Anything else is `revocations.malformed`. A missing bundle is `revocations.unsigned`. A bundle that does not verify is `revocations.unverifiable`.

**Importing it.** `install --index`, `update` and `hyalos plugin snapshot import <location>` read `revocations.json` from the location after the snapshot. They verify it under the snapshot now in force, so a newer snapshot from the same release is used. The file is kept in `trust/` under its digest, with the same flush-before-event rule as the snapshot, and the import is recorded as `plugin.revocations_imported { digest, signed_at }`. When the location has no list, nothing is said. A list that is no newer is not imported and nothing changes. `snapshot import` says so. A list that fails to verify is reported, and the list in force still applies. `snapshot show` prints the list in force: the event that imported it, its digest, its issue time and how many releases it revokes.

**Disabling what is installed.** After a list is imported, every installed release whose digest it lists is recorded as `plugin.revoked { name, digest, reason }`. A line tells the operator which release, why, and that no job is given it. `hyalos run --skill` refuses a revoked skill before anything is written. `plugin list` marks it revoked, with the reason and the event. Every later refresh applies the list in force again, so an import interrupted between the two events marks what it missed the next time. A plugin stays installed and disabled until a release that is not revoked is installed under its name. That install clears the mark.

**Refusing an install.** Every install checks the list in force, from an index or from a directory: a skill with the same bytes is the same release. A revoked digest is refused as `plugin.revoked`. `update` does not offer a revoked version, and records nothing waiting for it. If the kept list cannot be read back with the digest the log recorded, nothing is installed (`revocations.unreadable`), since what it revokes cannot be told. `snapshot import` from a location that holds a file with that digest puts it back, as for the snapshot.

**The engine.** Both events are written by the engine (`Origin::Engine`). They are not decisions. The maintainers decide a revocation in the marketplace repository, and the engine records that it applies here. The engine refuses:

- a list whose `signed_at` is not later than the last one's (`RevocationsNotNewer`);
- a `plugin.revoked` before any list was imported, for a release other than the one installed under the name, or for one already marked (`RevocationNotApplicable`);
- a `plugin.installed` of the release marked revoked under that name (`PluginRevoked`).

The state keeps the last list imported and the mark on each revoked plugin. The audit reports a mark for a release that is not installed. Schema version 20, state image 15.

No wire message or manifest changes.

## Consequences

- A release proven malicious stops being installed, and stops being given to jobs, on every host that next installs, updates or imports from the marketplace's releases. A host that never does keeps running it. The maximum age of a list (S7.6) is what makes an install refuse once its list is too old.
- The state does not hold the list's entries, only its digest. So the engine cannot check that a `plugin.revoked` matches the list. It can only check that it names the release installed. The CLI writes one only from a list it verified.
- A revocation cannot be undone by a list. To install the skill again, the maintainers publish other bytes under a new version.
- An air-gapped host gets the list in each release directory it imports, as ADR-0007 describes. That directory is the offline bundle, and #47 bounds its age.
- `snapshot import` now imports both files that travel with a release.
