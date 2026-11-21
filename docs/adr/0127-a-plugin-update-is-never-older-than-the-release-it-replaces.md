# ADR-0127: A plugin update is never older than the release it replaces

**Status:** Proposed

## Context

ADR-0090 resolves a plugin by name in an index, and takes the last version listed as the latest. ADR-0092's `hyalos plugin update` resolves each installed plugin at the latest version again and, when the install policy (ADR-0091) covers it, installs it by policy with no one asked. ADR-0093 verifies each release's Sigstore bundle, so only a release the pipeline signed installs, and ADR-0098 keeps a revoked release from installing again.

Issue #375 found that an update could still go backwards, to an older release that the pipeline signed and nobody revoked:

1. `index.json` is not signed, and "latest" is only its position in the list. Anyone who can rewrite the release assets or serve a mirror can move an older entry to the end.
2. The version recorded in `PluginSource::Index` came only from `index.json`. The signed archive did not carry it, so a mirror could also relabel the older release with a newer version string.
3. `update_one` checked only whether the digest differed from the one installed, and the revocation list. It never asked whether the release was newer. The signed time that `Trust::verify_signed` proves was thrown away.

The harm is bounded: the release must be one the pipeline signed, not revoked, and within the operator's ceiling. But an older, vulnerable release that was never revoked could replace a fixed one silently. ADR-0097 and ADR-0098 already refuse to go backwards for the trusted-root snapshot and the revocation list, by comparing their signed times. A plugin release had no such rule.

## Decision

**The signed time is kept.** An install from an index keeps the earliest signed time the release's Sigstore bundle proves (`Trust::verify_signed`, ADR-0093) and records it in the source: `{"index": {"location", "version", "signed_at"}}`. The field is optional and left out when empty, so a source recorded before reads as it was, with no signed time. It is a time the transparency log or a timestamp authority vouches for, never the local clock's.

**An update by policy must be strictly later.** `hyalos plugin update` refuses a candidate whose signed time is not later than the installed release's, with `plugin.not_newer`. Nothing is installed and no update is marked waiting; the installed version keeps running, and the check reports it and exits 1, as for any plugin it could not update. The engine holds the rule for every `plugin.installed` by policy, and fails closed: over an installed release with a signed time, the new source must have a strictly later one, or the event is refused (`RuleViolation::PluginNotNewer`). A source with no signed time is not known to be newer, so it is refused, and so are an equal time and an older one. Policy cannot install a local source at all (ADR-0091). Over an installed release with no signed time there is nothing to compare, so policy may install with or without one.

**A human may go older on purpose.** The engine does not check an install decided by the operator, signed with their key, against the signed time. The release it records carries its own signed time, so the next update by policy is checked against that one. A release not proven strictly later than the one installed is not covered by the install policy, so `hyalos plugin install --index <location> <name> --version <v>` takes the operator's path for it, by the same condition the engine applies (`PluginSource::policy_may_replace`): it needs their passphrase and is signed with their key. Neither `--version` nor `--grant` makes an install a human decision, and without the passphrase it is refused and writes nothing. A release that asks for more than is held still needs `--grant`. `hyalos plugin update` never asks for a human: it refuses such a release with `plugin.not_newer`.

**The version is bound to the archive.** A skill's `SKILL.md` front matter may carry `version:`, next to `name:`. Because `SKILL.md` is one of the archive's files, the version is inside what the pipeline signed. When it is there, an install or update from an index refuses the archive unless it is the version the index entry names (`plugin.wrong_version`), and refuses one that is not a `PluginVersion` (`plugin.bad_version`). A skill without one installs; its version is then only the index's.

**A malformed version matters only to an index.** Reading, storing and loading a skill, and installing one from a directory, accept a malformed `version:`, so a skill that worked before this rule is never reported as altered. The skill keeps only that its declaration is absent, valid or malformed (`Skill::declared_version`), never the malformed text, which is the skill's own and untrusted. The archive and its digest are unchanged.

**The stored forms change.** This is schema version 25 (`plugin.installed`'s index source gains `signed_at`; an earlier event has none and reads as it was) and `StateImage` version 18 (the same source in `plugins`; an older image's index plugin reads as having no signed time). Neither needs an upcast that supplies a value, because the field defaults to none; the image's 17 to 18 step says so explicitly and keeps a time already there.

## Consequences

- An update by policy can no longer replace a release with an older signed one, however `index.json` is ordered or labelled. The anti-rollback rule is the same one ADR-0097 and ADR-0098 use, applied per plugin.
- A skill that declares its version can no longer be relabelled by its index. One that does not is protected by its signed time alone, which is enough to stop a rollback but not a wrong label in `plugin list`. That gap remains and is tracked in its own issue.
- Logs and images written before this change have no signed time for any installed plugin, so the first update after upgrading is not checked against one. It is still a release the pipeline signed, not revoked and within the ceiling, and it records a signed time that every later update is checked against.
- Once a plugin has a signed time, policy can never replace it with a source that lacks one. Every install from an index this build writes carries a time, so no install the command makes meets this refusal. A plugin with no signed time (none over none, from 0.1.8 or earlier) stays updatable by policy, and existing logs replay as they were.
- The operator can roll back a bad release, or reinstall one, while the install policy covers it, without loosening the policy. It takes their signature, so a driving agent without the passphrase cannot roll a skill back through this command. After a rollback, the next update by policy is checked against the signed time of the release rolled back to, so a later release installs again unless the operator changes the policy or the index.
- `index.json` itself is still unsigned. A mirror can still freeze it, hiding a newer release, or drop entries; it cannot make an update go backwards. Signing the index and refusing an older one (the issue's optional third item) is not done.
- Pinned: `core/types/tests/schema.rs` (version 25's `plugin.installed` by policy, with a signed time) and `core/types/tests/snapshot.rs` (version 18's index plugin with one).
