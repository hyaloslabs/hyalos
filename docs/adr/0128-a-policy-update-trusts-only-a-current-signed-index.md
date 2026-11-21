# ADR-0128: A policy update trusts only a current signed index

**Status:** Accepted

## Context

ADR-0127 stopped a policy update from installing an older signed release, and left two gaps that issue #378 tracks:

1. The version shown and recorded for a release comes from `index.json`, which was unsigned. The archive binds a version only if its `SKILL.md` declares one, so a mirror could relabel a release that does not.
2. A mirror could freeze or reorder `index.json`. It could not make an update go backwards, but it could withhold a newer release, such as one that fixes a vulnerability, and the operator would see nothing.

The marketplace's release pipeline (ADR-0094) signs each archive, the trusted-root snapshot (ADR-0097) and the revocation list (ADR-0098). It did not sign the index. Releases `r1` to `r10` have no signature for theirs, and an offline bundle (ADR-0099) may not either.

## Decision

**The pipeline signs the index.** Every release carries `index.json.sigstore.json`, signed by the same workflow identity as everything else. The index names each version with its archive's digest, so signing it binds every version label to the bytes it describes, whether or not the skill declares a `version:`. The release that carries the snapshot is published at least every 7 days and signs the index again, even when its contents have not changed.

**An update by policy needs a current signed index.** `hyalos plugin update`, and an install the install policy would decide, use an index only if:

- its bundle verifies for the pipeline's identity under the trusted-root snapshot in force (`index.unsigned`, `index.unverifiable`);
- it is no older than the index accepted last from that location, and is not a different file with the same signed time (`index.not_newer`);
- it was signed at most 30 days ago (`index.too_old`), which is how a frozen index is detected. The limit is `MAX_INDEX_AGE_DAYS`, the default age of a revocation list, and about four of the pipeline's re-signing periods.

The bytes checked are the bytes resolved: the index is fetched once. A refusal leaves the installed version running and the check exits 1, as for any plugin it could not update.

**The newest index is recorded per location.** A later signed index is recorded as `plugin.index_accepted` with its location, digest and signed time. It is not a decision: the engine writes it, and refuses one no later than the last at that location. Recording is per location because a location pinned to an old release is a legitimate source and must not be refused for being older than a different location's index. An update records it even if the release it names is then refused, because the index itself is genuine. An install records it only when the install goes ahead. A file is recorded only if it reads as an index: the pipeline signs its other files with the same identity, so a mirror serving one of those as `index.json` is refused (`index.malformed`) and cannot raise the location's floor.

**The operator may still install from an older index.** An install the operator signs can use an index that is unsigned, older than the last accepted or past the age limit, so older releases and offline bundles remain installable. The command says that the version is unverified before it asks for the passphrase, policy cannot make that install, and the index is not recorded as accepted. A signature that is present and fails is refused for the operator too, because that index has been altered.

**The stored forms change.** This is schema version 26 (`plugin.index_accepted`, changing no existing payload) and `StateImage` version 19 (`accepted_indexes`; an older image has none).

## Consequences

- A mirror can no longer relabel a release, replay an older index or strip the signature without an update reporting it with a reason code.
- A mirror that freezes on a current index can still hide a newer release for up to 30 days. After that, updates from it stop and say why.
- A marketplace outage longer than 30 days stops policy updates, and installed versions keep running. The operator can still install by hand.
- An installed plugin whose location is pinned to a release tag stops updating by policy once that index is 30 days old, or at once if it has no signature. The operator reinstalls from a current location.
- The first update after upgrading accepts whichever current signed index it finds, since no earlier one is recorded.
- The unverified state of a version is reported when the operator installs it. It is not stored, so `plugin list` does not show it.
- Pinned: `core/types/tests/schema.rs` (version 26), `core/types/tests/snapshot.rs` (version 19), and the marketplace's real signed index from release `r18` in `core/plugin/tests/sigstore/marketplace-r18/`.
