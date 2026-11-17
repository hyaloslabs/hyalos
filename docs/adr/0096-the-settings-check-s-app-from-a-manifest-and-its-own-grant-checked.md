# ADR-0096: The settings check's App, from a manifest, with its own grant checked

**Status:** Accepted

## Context

[ADR-0095](0095-checking-the-marketplace-s-protection-settings.md) has the marketplace's daily settings check read through a GitHub App of its own. The App is owned by `hyaloslabs`, installed on `hyaloslabs/marketplace` alone, and has Actions, Administration, Metadata and the organization's Members read-only. A maintainer made it by hand, ticking each permission. Two gaps remain:

- A wrong tick when the App is made, Administration: write say, goes unnoticed. ADR-0095 accepts that "a permission added to the App is not noticed": each run's token asks for the four read permissions only, so a wider App would not widen the token, and nothing compared the App's own grant. The same goes for the App later being installed on more repositories.
- The steps live only in the marketplace's README, and nothing in `docs/operations/` covers running the marketplace. Hyalos's own App already has a setup page that creates it from a fixed manifest ([ADR-0084](0084-setup-creates-the-github-app-from-a-manifest.md)).

This is for hyaloslabs maintainers. Nobody installing Hyalos runs the marketplace, so none of it reaches `hyalos setup` or the setup skill.

## Decision

**A fixed manifest and a page beside Hyalos's.** `site/setup/settings-check/manifest.json` sets the four read permissions, no events, a private App and an inactive webhook at Hyalos's site. Its `redirect_url` is the page, `https://hyaloslabs.github.io/hyalos/setup/settings-check/`. The page posts the manifest to `hyaloslabs`'s new-App form. The maintainer may rename the App there, and nothing else. It loads nothing from elsewhere, and its Content-Security-Policy, like ADR-0084's, sends forms only to github.com. A test in `core/hyalos/tests` pins the manifest's permissions, events and addresses.

**The key goes from GitHub into the environment, through the maintainer's own terminal.** Back on the page, GitHub's one-time code is shown and removed from the address bar, with a command for a terminal where `gh` is logged in as an admin. The command:
- reads the code as hidden input;
- converts it (`POST /app-manifests/{code}/conversions`);
- stores the private key as the secret `SETTINGS_APP_KEY` and the client ID as the variable `SETTINGS_APP_CLIENT_ID` in the `settings-check` environment, piping the key into `gh secret set` and writing nothing to disk.

It runs as one subshell, so the code and GitHub's answer go when it ends. It stores nothing unless the App is owned by `hyaloslabs` and has exactly the manifest's permissions and no events, and then says to delete the App. The page never receives, shows or forwards the key, and the test pins those checks in the command. Installing the App, on the marketplace alone, stays a step the maintainer takes on GitHub.

**The check reads the App's own grant, as the App.** `check_settings.py` signs a ten-minute JWT with the App's key, using `openssl`, which gets the key through a pipe only. With the JWT it reads:
- the App's own permissions, events and owner (`GET /app`);
- each installation's account, repository selection, permissions and events (`GET /app/installations`);
- each installation's repositories, through a token asking for Metadata read alone, revoked as soon as they are listed.

`settings/expected.json` gains an `app` section with all of it, compared like every other setting. So a wider App, one installed elsewhere, or one subscribed to events is drift, and stops releases as ADR-0095 describes. Without the App's key and client ID the comparison fails, before any request is made. `--print` without them keeps the file's `app` section and says so. The key reaches only the check job's compare step, which `check_workflows.py` already limits it to. Neither the key nor openssl's output is printed.

**One runbook.** `docs/operations/marketplace.md` covers making the App, from the page or by hand; storing its key and client ID; the first run; rotating the key; and changing a setting on purpose. The marketplace README points to it.

This supersedes ADR-0095's account of the App's grant. Nothing else in ADR-0095 changes. No event, wire message, manifest or state changes in Hyalos's engine.

## Consequences

- A permission added to the App, an installation on another repository, or an event subscription is reported within a day, and releases stop until it is undone or recorded in `settings/expected.json`.
- The App can be made with one form, and the command stores nothing for an App other than the one the page asked for.
- The check now holds the App's key in its compare step as well as in the step that mints the run's token. Every reader there is code in the marketplace, behind its own ruleset. The key could already mint any token the App can.
- Each run makes one more short-lived token per installation: Metadata read, revoked within seconds.
- Changing the App's permissions on purpose takes three edits in one review: the manifest here, `settings.yml`'s `permission-*` inputs, and `settings/expected.json`.
