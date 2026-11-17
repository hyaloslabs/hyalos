# ADR-0097: The trusted-root snapshot the pipeline signs, and importing a newer one

**Status:** Accepted

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) says every install verifies a release's Sigstore bundle against a trusted-root snapshot: Sigstore's trusted root, republished by the marketplace pipeline after fetching it through TUF, and signed by the pipeline's identity so that it carries a signed issue time. Each Hyalos release embeds the current one. A newer one is accepted only if it verifies under the one installed and is newer than it, and if the installed one cannot verify its successor, the install refuses it and needs a new Hyalos release.

[ADR-0093](0093-verifying-a-plugin-release-s-sigstore-bundle.md) built the check against `core/plugin/trust/trusted_root.json`, Sigstore's root copied in by hand, and left the snapshot to S7.11 (#105). [ADR-0094](0094-the-marketplace-release-pipeline.md) built the pipeline. Its `check_workflows.py` lets `release.yml` run only on a push to `main`, so nothing can run it on a schedule, and its releases carry no snapshot.

## Decision

**The snapshot file.** `trusted-root.json` is

```json
{"kind": "hyalos.trusted_root_snapshot", "trusted_root": {...}, "version": 1}
```

with Sigstore's trusted root inside, and `trusted-root.json.sigstore.json` beside it is the pipeline's Sigstore bundle for those bytes. The `kind` keeps a snapshot from being read as anything else the pipeline signs, and the reverse: a skill archive is not JSON of this kind, and a snapshot is not an archive. The issue time is the earliest signed time the bundle proves (the log's integrated time, or a timestamp authority's). The file holds no time of its own, so nothing unsigned can claim one.

**The pipeline (in `hyaloslabs/marketplace`).**

- `tools/snapshot.py` fetches `trusted_root.json` through Sigstore's TUF repository with python-tuf, installed from `tools/requirements-snapshot.txt` by hash and from wheels only. Trust starts at TUF root version 16, pinned in `trust/sigstore-tuf-root.json`, and python-tuf walks forward through each newer root. It refuses a root that does not chain, metadata that expired or rolled back, and a target whose length or hashes are not the ones the signed targets metadata gives. The script then checks the result is a trusted root, listing logs, certificate authorities and CT logs, and wraps it. The same root always gives the same bytes.
- `release.yml` also runs daily on a `schedule`. GitHub runs a scheduled workflow from the default branch's own file, with `refs/heads/main` as its ref, so a scheduled run signs as the identity Hyalos pins (`PIPELINE`) and no new identity is needed. `check_workflows.py` now allows `release.yml` a push to `main` and a `schedule` of cron lines, and nothing else. A `workflow_dispatch`, `workflow_call` or pull-request trigger is still refused. This extends ADR-0094's rule on triggers and changes nothing else in it.
- A `snapshot` job fetches it, with no token. `sign` signs it with the archives and verifies its bundle for the pinned identity, and against another identity, which must fail. `publish` adds it to the release. A release is published when the index changed, the trusted root changed, or the last release is 7 days old, so what the latest release carries was signed at most about a week before. That leaves room under the 90 days a snapshot may be old, and under the 30 days a revocation list may be (S7.6, S7.7).
- A run that fails publishes nothing, and the last release stays current. An `alert` job, the only one with `issues: write`, opens one `release-failed` issue, or comments on the open one.

**What a Hyalos release embeds.** `core/plugin/trust/trusted-root.json` and its bundle, as a release downloaded them. Each time Hyalos loads it, it checks that the embedded bundle verifies under the embedded root for `PIPELINE`, and that signed time is the embedded snapshot's issue time. A test pins that time and the root. The first one is release `r8`'s, which holds the same root as the hand-copied `trusted_root.json` it replaces.

**The snapshot in force** is the newest of the embedded one and the last one imported, by issue time. An imported snapshot is kept in the data directory's `trust/` directory under its digest, and read back only if its bytes still have the digest the log recorded. It is flushed to disk, with its directory, before the event that names it is written. If it cannot be read, installs from an index are refused (`snapshot.unreadable`). They never fall back to the older embedded snapshot. `snapshot import` from a location that holds a file with the digest the log recorded, such as the release it came from, puts it back. Only the subcommands that check a release (`install --index`, `update`, `snapshot`) load the embedded snapshot, so `list`, `pack`, `policy` and local installs never depend on it.

**Importing a newer one.** `hyalos plugin snapshot import <location>` takes an index location (an `https://` URL or a directory, as `install --index` does) and reads `trusted-root.json` and its bundle from it. It is accepted only if:

- the bundle verifies, under the snapshot in force and for `PIPELINE`, with ADR-0093's full check;
- its issue time is later than the issue time of the snapshot in force;
- it is a snapshot of version 1, at most 1 MiB, whose trusted root lists logs, certificate authorities and CT logs and can be used by this build. A root that lists none parses in the library, but would verify nothing, so it is refused as `snapshot.unusable`.

If the bundle does not verify, the import is refused with `snapshot.unverifiable`, and the message says that a new Hyalos release is needed if the pipeline signed it. That is ADR-0007's rotation case: Sigstore changed so that the installed snapshot cannot verify its successor. A snapshot that is no newer is refused as `snapshot.not_newer`. A bundle that is missing is refused as `snapshot.unsigned`. An accepted import is recorded as `plugin.snapshot_imported`.

`install --index` and `update` try the same import from their location first. If the location carries a newer snapshot that verifies, it is imported and the install proceeds under it. If it carries none (absent from the directory, or a 404), or one that is no newer, nothing is said. If one fails to verify, the output says so and the install proceeds under the snapshot in force. A location cannot make the check weaker, only newer.

`hyalos plugin snapshot show` prints the snapshot in force: embedded or imported at an event, its digest and its issue time.

**The event.** `plugin.snapshot_imported { digest, signed_at }`, written by the engine (`Origin::Engine`). It is not a decision: the operator's trust in the pipeline was decided when they installed this Hyalos release. The state keeps the last one. The engine refuses one whose `signed_at` is not later than the last one's (`snapshot.not_newer`), so the log never records a rollback. Schema version 19, state image 14.

No wire message or manifest changes.

## Consequences

- An install verifies against Sigstore's trusted root as the pipeline last saw it. It needs neither a Hyalos release for each change to Sigstore's root nor any network beyond the index location.
- Sigstore's TUF metadata does not travel into an air gap. The pipeline's signed snapshot does, inside a downloaded release directory, which is an offline bundle (ADR-0090).
- The pipeline's identity now vouches for two kinds of file. `kind` and the archive format keep them apart, and Hyalos reads a snapshot only through this import.
- A scheduled run is a second way `release.yml` runs, and it still signs only what `main` holds. Each scheduled run that publishes writes a Rekor entry naming the repository, as ADR-0094 already accepted.
- If Sigstore rotates so that the snapshot in force cannot verify the next one, imports stop and installs continue under the old snapshot. They refuse bundles from any new authority, and fail closed once the snapshot is too old (S7.6), until a Hyalos release embeds a newer one.
- The maximum age of a snapshot (S7.6) is measured from this issue time.
