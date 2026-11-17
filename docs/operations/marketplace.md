# The marketplace: its settings check and the App it reads with

For hyaloslabs maintainers. Nobody installing Hyalos needs this page. `hyaloslabs/marketplace` signs its releases keylessly, so what reaches its `main` is what Hyalos trusts ([ADR-0094](../adr/0094-the-marketplace-release-pipeline.md)). The settings that decide what reaches `main` are checked every day against `settings/expected.json` in that repository ([ADR-0095](../adr/0095-checking-the-marketplace-s-protection-settings.md)). The check reads through a GitHub App of its own, and checks that App's grant too ([ADR-0096](../adr/0096-the-settings-check-s-app-from-a-manifest-and-its-own-grant-checked.md)). Releases are refused while the check has not passed within 8 days or a `settings-drift` issue is open.

You need `gh` logged in as a `hyaloslabs` owner, and `jq`.

## 1. Create the App

### From the setup page

1. Open `https://hyaloslabs.github.io/hyalos/setup/settings-check/` and choose **Continue to GitHub**. GitHub shows the App it will make in `hyaloslabs`, with no webhook and only these permissions:
   - Repository: **Actions**, **Administration** and **Metadata**, read-only
   - Organization: **Members**, read-only

   You may rename it. Choose **Create GitHub App**.
2. GitHub sends you back to the page, which shows a one-time code and a command. Run the command in a terminal of your own and paste the code when it asks. It is not shown as you type, and it works once, within an hour.
3. The command fetches the App's key and client ID from GitHub. It stores them only if the App is owned by `hyaloslabs` and has exactly the permissions above and no events. Otherwise it says to delete the App. When the App is right, it stores the key as the secret `SETTINGS_APP_KEY` and the client ID as the variable `SETTINGS_APP_CLIENT_ID`, both in the marketplace's `settings-check` environment. Nothing is written to disk, and the key never reaches the page.

### By hand

1. Organization settings ▸ Developer settings ▸ GitHub Apps ▸ **New GitHub App**, for example `hyaloslabs-settings-check`. Turn off the webhook. Set the permissions above, nothing else, and **Only on this account**.
2. On the App's settings page, generate a private key. Store its whole contents as the secret `SETTINGS_APP_KEY`, and the **Client ID** as the variable `SETTINGS_APP_CLIENT_ID`. Both go in the marketplace repository ▸ Settings ▸ Environments ▸ **settings-check**, not under the repository-wide Actions secrets, which are not limited to `main`. Delete the downloaded key file.

## 2. Install it

From the App's page, **Install App** ▸ `hyaloslabs` ▸ **Only select repositories** ▸ `hyaloslabs/marketplace`. Install it on nothing else: the check compares the repositories it covers with `settings/expected.json`, so any other one is drift.

## 3. Run the check

```
gh workflow run settings.yml --repo hyaloslabs/marketplace --ref main
gh run watch --repo hyaloslabs/marketplace "$(gh run list --repo hyaloslabs/marketplace --workflow settings.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

It passes with "the settings are as expected", and closes any open `settings-drift` issue. A failure opens or updates that issue with what differed or could not be read. The check prints settings and HTTP statuses, never a token, the key or a response body.

## Rotating the key

The key does not expire. To replace it: on the App's settings page, generate a new private key, store it as `SETTINGS_APP_KEY` as above, run the check, then delete the old key there. If you think the key has leaked, delete it first. The check fails until the new one is stored, which is safe: releases wait.

## Changing a setting on purpose

Change it in GitHub, then update `settings/expected.json` in a pull request with the live settings, read with your own admin token:

```
out=$(SETTINGS_READ_TOKEN=$(gh auth token) python3 tools/check_settings.py hyaloslabs/marketplace --print) && printf '%s\n' "$out" > settings/expected.json
```

The file is written only when the read succeeded, so a failed one leaves it as it was.

Without the App's key, `--print` keeps the file's `app` section and says so. Edit that section by hand when the App changes. A ruleset's bypass list is visible only to a token that can write rulesets, so the check pins each ruleset's change time instead. Any edit to a ruleset is drift until the file is updated, even saving it unchanged.

To change the App's permissions, change all three in one pull request: `site/setup/settings-check/manifest.json` and its test here, `settings.yml`'s `permission-*` inputs, and the `app` section of `settings/expected.json`. Then update the App on GitHub and accept the change on the installation.
