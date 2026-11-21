# ADR-0127: A plugin update is never older than the release it replaces

**Status:** Accepted

## Context

ADR-0090 resolves a plugin by name in an index, and takes the last version listed as the latest. ADR-0092's `hyalos plugin update` resolves each installed plugin at the latest version again and, when the install policy (ADR-0091) covers it, installs it by policy with no one asked. ADR-0093 verifies each release's Sigstore bundle, so only a release the pipeline signed installs, and ADR-0098 keeps a revoked release from installing again.

Issue #375 found that an update could still go backwards, to an older release that the pipeline signed and nobody revoked:

1. `index.json` is not signed, and "latest" is only its position in the list. Anyone who can rewrite the release assets or serve a mirror can move an older entry to the end.
2. The version recorded in `PluginSource::Index` came only from `index.json`. The signed archive did not carry it, so a mirror could also relabel the older release with a newer version string.
3. `update_one` checked only whether the digest differed from the one installed, and the revocation list. It never asked whether the release was newer. The signed time that `Trust::verify_signed` proves was thrown away.

The harm is bounded: the release must be one the pipeline signed, not revoked, and within the operator's ceiling. But an older, vulnerable release that was never revoked could replace a fixed one silently. ADR-0097 and ADR-0098 already refuse to go backwards for the trusted-root snapshot and the revocation list, by comparing their signed times. A plugin release had no such rule.

## Decision

**The signed time is kept.** An install from an index keeps the earliest signed time the release's Sigstore bundle proves (`Trust::verify_signed`, ADR-0093) and records it in the source: `{"index": {"location", "version", "signed_at"}}`. The field is optional and left out when empty, so a source recorded before reads as it was, with no signed time. It is a time the transparency log or a timestamp authority vouches for, never the local clock's.

**An update by policy must be strictly later.** `hyalos plugin update` refuses a candidate whose signed time is not later than the installed release's, with `plugin.not_newer`. Nothing is installed and no update is marked waiting; the installed version keeps running, and the check reports it and exits 1, as for any plugin it could not update. The engine holds the same rule for every `plugin.installed` by policy: when both the installed source and the new one carry a signed time, the new one must be later, or the event is refused (`RuleViolation::PluginNotNewer`). Equal times are refused too, since a different digest at the same time is not an update.

**A human may go older on purpose.** An install by the operator, signed with their key, is not checked against the signed time. Rolling back to a known release is a decision only the operator makes, by naming it: `hyalos plugin install --index <location> <name> --version <v>`. The release it records carries its own signed time, so the next update by policy is checked against that one.

**The version is bound to the archive.** A skill's `SKILL.md` front matter may carry `version:`, next to `name:`. Because `SKILL.md` is one of the archive's files, the version is inside what the pipeline signed. When it is there, an install from an index refuses the archive unless it is the version the index entry names (`plugin.wrong_version`). A `version:` that is not a `PluginVersion` makes the skill unreadable (`plugin.bad_version`). A skill without one is still read; its version is then only the index's.

**The stored forms change.** This is schema version 25 (`plugin.installed`'s index source gains `signed_at`; an earlier event has none and reads as it was) and `StateImage` version 18 (the same source in `plugins`; an older image's index plugin reads as having no signed time). Neither needs an upcast that supplies a value, because the field defaults to none; the image's 17 to 18 step says so explicitly and keeps a time already there.

## Consequences

- An update by policy can no longer replace a release with an older signed one, however `index.json` is ordered or labelled. The anti-rollback rule is the same one ADR-0097 and ADR-0098 use, applied per plugin.
- A skill that declares its version can no longer be relabelled by its index. One that does not is protected by its signed time alone, which is enough to stop a rollback but not a wrong label in `plugin list`.
- Logs and images written before this change have no signed time for any installed plugin, so the first update after upgrading is not checked against one. It is still a release the pipeline signed, not revoked and within the ceiling, and it records a signed time that every later update is checked against.
- The engine's rule applies only when both sources carry a signed time. Every install from an index this build writes carries one, so the gap is only for a plugin installed before the upgrade.
- `index.json` itself is still unsigned. A mirror can still freeze it, hiding a newer release, or drop entries; it cannot make an update go backwards. Signing the index and refusing an older one (the issue's optional third item) is not done.
- Pinned: `core/types/tests/schema.rs` (version 25's `plugin.installed` by policy, with a signed time) and `core/types/tests/snapshot.rs` (version 18's index plugin with one).
