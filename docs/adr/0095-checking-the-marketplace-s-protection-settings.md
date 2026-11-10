# ADR-0095: Checking the marketplace's protection settings

**Status:** Proposed

## Context

[ADR-0094](0094-the-marketplace-release-pipeline.md) signs marketplace releases keylessly. There is no key: whatever reaches `main` in `hyaloslabs/marketplace` is what `release.yml` signs as the identity Hyalos pins. So the settings that decide what reaches `main`, and what a workflow there may do, are the protection of the one trust anchor [ADR-0007](0007-capability-based-marketplace-trust.md) names:

- the `main` ruleset: pull requests only, with a code owner's review and CI passing, no force push or deletion, and an admin bypass only through a pull request;
- the Actions settings: workflows default to read-only tokens and may not approve pull requests, and no other repository may use these workflows;
- who holds admin, the visibility, and forking.

They live in GitHub, not in either repository, and nothing noticed one being loosened. ADR-0094 left this check to its own story (S7.13, #304). hivrr had a scheduled drift check for its marketplace for the same reason.

## Decision

**Expected settings live in the repository.** `settings/expected.json` holds what the settings above must be, plus the environment the check's token sits in. `tools/check_settings.py --print` writes it from the live settings. A setting changed on purpose is changed in GitHub and in that file, in one pull request, so the change is reviewed where the code is.

**A daily read-only check.** `settings.yml` runs on a schedule and on demand. It reads every ruleset (rules, conditions, bypass list), the Actions settings, admins, the repository's visibility and forking, and each environment with its branch rule. It compares them with the file and names each difference. A setting it cannot read, including a bypass list GitHub hides from a reader without admin read, is an error, never a match.

**One issue, opened and closed by the check.** On drift, the check opens one issue labelled `settings-drift` with the differences, or comments on the open one. It closes the issue when the settings match again.

**A stopped check blocks the next release.** A scheduled workflow that stops running reports nothing, so no check inside it can notice that. Instead `release.yml` gains a `settings` job that `sign` needs. It passes only if the last finished settings check on `main` succeeded within the last 8 days and no drift issue is open. A check that stopped, failed or found drift therefore stops signing, which is the moment it matters, and the release run says why.

**Its token.** Some of these settings are visible only to a reader with admin read, which a workflow's own token never has. The check uses `SETTINGS_READ_TOKEN`:

- It is a fine-grained token for that repository alone, with Administration and Metadata read-only. The operator creates it and renews it; no agent reads it.
- It sits in the `settings-check` environment, which only `main` may use, so a pull request's workflow cannot reach it.
- `tools/check_workflows.py` fails if any job but `settings.yml`'s `check` names the token or the environment, or if `settings.yml` gains a trigger other than its schedule and manual runs.
- The check prints settings and HTTP statuses, never the token or a response body.

No event, wire message, manifest or state changes in Hyalos.

## Consequences

- Loosening a protection the trust rests on is reported within a day and stops releases until it is undone, or recorded in `settings/expected.json` through a reviewed pull request.
- Releases depend on the operator's token. One that expires stops the check, then signing after 8 days, with a message saying so. That is the fail-closed reading, at the cost of a renewal to remember.
- The check covers the repository's own settings. Organization rulesets appear among the repository's rulesets and are checked too. Organization membership and organization-wide Actions policy are not: an organization owner could still act beyond what it sees.
- A change made and reverted between two daily checks is not seen. The window is a day.
