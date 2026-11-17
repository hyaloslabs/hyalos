# ADR-0095: Checking the marketplace's protection settings

**Status:** Accepted

## Context

[ADR-0094](0094-the-marketplace-release-pipeline.md) signs marketplace releases keylessly. There is no key: whatever reaches `main` in `hyaloslabs/marketplace` is what `release.yml` signs as the identity Hyalos pins. So the settings that decide what reaches `main`, and what a workflow there may do, are the protection of the one trust anchor [ADR-0007](0007-capability-based-marketplace-trust.md) names:

- the `main` ruleset: pull requests only, with a code owner's review and CI passing, no force push or deletion, and an admin bypass only through a pull request;
- the Actions settings: workflows default to read-only tokens and may not approve pull requests, and no other repository may use these workflows;
- who holds admin, the visibility, and forking.

They live in GitHub, not in either repository, and nothing noticed one being loosened. ADR-0094 left this check to its own story (S7.13, #304). hivrr had a scheduled drift check for its marketplace for the same reason.

## Decision

**Expected settings live in the repository.** `settings/expected.json` holds what the settings above must be, plus the environment the check's token sits in. `tools/check_settings.py --print` writes it from the live settings. A setting changed on purpose is changed in GitHub and in that file, in one pull request, so the change is reviewed where the code is.

**A daily read-only check.** `settings.yml` runs on a schedule and on demand. It reads every ruleset (rules, conditions, and when it last changed), the Actions settings, admins, the repository's visibility and forking, and each environment with its branch rule. It compares them with the file and names each difference. A setting it cannot read is an error, never a match.

**A ruleset's bypass list is covered by its change time.** GitHub returns a ruleset's bypass list only to a reader that can write to rulesets. A read-only token never sees it, so comparing it would need a token that could loosen the protection being checked. Instead the file pins each ruleset's `updated_at`, in UTC. Any edit to a ruleset moves it, so a widened bypass list is drift, though the check can name only the ruleset, not the entry. A maintainer reads the change with an admin's token (`--print`). Where the token can see the bypass list, it is compared too. A ruleset whose change time is missing or unreadable is an error, and so is a repository with no admin visible, since it always has one.

**One issue, opened and closed by the check.** On drift, the check opens one issue labelled `settings-drift` with the differences, or comments on the open one. It closes the issue when the settings match again.

**A stopped check blocks the next release.** A scheduled workflow that stops running reports nothing, so no check inside it can notice that. Instead `release.yml` gains a `settings` job that `sign` needs. It passes only if the last finished settings check on `main` succeeded within the last 8 days and no drift issue is open. A check that stopped, failed or found drift therefore stops signing, which is the moment it matters, and the release run says why.

**Its token comes from an app of its own.** Some of these settings are visible only to a reader with admin read, which a workflow's own token never has. Neither app already installed on the organization fits: Hyalos's app cannot read Administration, and giving its key to a workflow would put a key that can push to `main` in the repository whose `main` this check protects. A fine-grained personal token could read as much, but it belongs to one person and expires, which would stop releases on a missed renewal. So the check has its own GitHub App:

- It is owned by the organization, installed on that repository alone, and has Actions, Administration and Metadata read-only, and the organization's Members read-only, and nothing else. Actions is there because GitHub lists environments and their branch rules under it. Members is there because organization owners are admins of the repository, and an app sees them among its collaborators only with it. The operator creates it and holds its key; no agent reads it.
- Each run mints a token from it with `actions/create-github-app-token`, pinned by commit, asking for those four permissions only. The token lasts an hour and is revoked when the job ends.
- The app's client ID (variable `SETTINGS_APP_CLIENT_ID`) and private key (secret `SETTINGS_APP_KEY`) sit in the `settings-check` environment, which only `main` may use, so a pull request's workflow cannot reach them.
- `tools/check_workflows.py` fails if any job but `settings.yml`'s `check` names the key, the client ID, the minted token or the environment, or if `settings.yml` gains a trigger other than its schedule and manual runs.
- A token that cannot be minted is a failed check, reported in the drift issue. The check prints settings and HTTP statuses, never the token, the key or a response body.

No event, wire message, manifest or state changes in Hyalos.

## Consequences

- Loosening a protection the trust rests on is reported within a day and stops releases until it is undone, or recorded in `settings/expected.json` through a reviewed pull request.
- Releases depend on the check's app. Nothing expires, so there is no renewal to forget. If the app is uninstalled, its key deleted or a permission taken away, the check fails, and signing stops after 8 days, with a message saying so. A permission added to the app is not noticed. The token still asks for only the four, but nothing compares the app's own grant (#307).
- The app's key does not expire. If it leaked, it would let someone read these settings and nothing more, until the operator deletes it in the app's settings.
- The check covers the repository's own settings. Organization rulesets appear among the repository's rulesets and are checked too. Organization membership and organization-wide Actions policy are not: an organization owner could still act beyond what it sees.
- A change made and reverted between two daily checks is not seen. The window is a day.
- Any edit to a ruleset is reported, even one that changes nothing, and the report names the ruleset rather than the entry that changed. Every intended edit is followed by a pull request updating `settings/expected.json`.
