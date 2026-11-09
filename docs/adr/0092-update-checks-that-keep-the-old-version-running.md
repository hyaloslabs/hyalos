# ADR-0092: Update checks that keep the old version running

**Status:** Proposed

## Context

[ADR-0007](0007-capability-based-marketplace-trust.md) says updates within the operator's install policy install automatically, and an update that adds a capability beyond it waits for a human. S7.10 (#83) asks for the engine to check installed plugins for updates on a schedule, for an update within policy to install, and for one that adds a capability to stay pending while the installed version keeps running until a human consents.

[ADR-0090](0090-a-plugin-index-served-as-release-assets.md) records where each index install came from: its index and version. [ADR-0091](0091-the-install-policy-a-capability-ceiling-the-operator-signs.md) defines the install policy and lets `policy` install what it covers.

Hyalos has no daemon of its own. `hyalos mcp` runs for a long time, but only while an agent uses it, and its part of the line budget is nearly spent. A check is short and needs no state between runs.

## Decision

**A check is a command.** `hyalos plugin update` takes each plugin installed from an index and does this:

1. It resolves the plugin's name in that index again, at the latest version, and fetches and checks the archive as an install does (digest, then name).
2. If the digest is the one installed, the plugin is up to date.
3. If the install policy covers the new version (a listed index, nothing beyond the ceiling), it is installed by `policy`. The new `plugin.installed` replaces the old one, and the next job given the skill gets the new files.
4. Otherwise, the engine writes `plugin.update_pending { name, version, digest, asks, location }`. Nothing is installed: the installed version and its grant stay in force.

A check that finds the same pending digest again writes nothing. A check that cannot reach or read an index reports it, writes nothing for that plugin, and exits 1. The others are still checked.

**Pending is a fact, not a decision.** `plugin.update_pending` comes only from the engine, never the broker. The state accepts it only for a plugin installed from an index, with a grant in the vocabulary. The state keeps the latest one for each name (`State::pending_updates`; `StateImage` version 13). Any install of the name clears it: the operator's, with `--grant` after reading the diff, or a later install by policy.

`hyalos plugin list` shows each waiting update and what it asks for, with the command that installs it. Consenting is `hyalos plugin install --index <location> <name> --grant`, the same decision as any install. No separate approval event is needed.

**The schedule is the command's.** `hyalos plugin update --every <hours>` (1 to 168) checks, prints what it did, sleeps, and checks again until stopped. A failed check does not stop the schedule. It runs as a service or in a terminal, as the operator chooses. A systemd timer or cron job running `hyalos plugin update` does the same. The check needs no passphrase, because it can write only installs by policy and pending updates.

This is schema version 18 (`plugin.update_pending`, no payload changed) and `StateImage` version 13 (`pending_updates`).

## Consequences

- Within the policy, a published update reaches jobs at the next check, with no one asked.
- An update beyond the policy never displaces the version in force. It waits in the log and in `plugin list` until the operator installs it.
- A check can only install what ADR-0091 already lets policy install, so a compromised listed index gains nothing from the schedule that it could not get from one install.
- Running checks is up to the operator, so a host that never runs one never updates. `hyalos doctor` does not yet say when the last check ran.
