# ADR-0084: Setup creates the GitHub App from a manifest

**Status:** Accepted. Supersedes [ADR-0080](0080-setup-asks-the-operator-in-their-own-terminal.md)'s account of `github-app`, for an App not set up yet.

## Context

[ADR-0080](0080-setup-asks-the-operator-in-their-own-terminal.md) has `hyalos setup github-app` print five lines on how to create the App in a browser, ask for its App ID, and find its `.pem` wherever it was saved. In three timed runs on bot1 (#253), getting the `.pem` was the hardest step. A browser download lands on the computer the browser runs on, and on a machine reached over SSH it has to be copied across with `scp`, which most operators would not know to do (#275). Setting the App's fields by hand also took most of the 13 minutes measured in the first run, and the prefilled link broke when it was copied.

GitHub's [App manifest flow](https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest) creates an App from a manifest and hands its id and private key back in one exchange (#267). It has three constraints:
- The manifest reaches GitHub only as a form POST of a `manifest` field, to `github.com/settings/apps/new` or `github.com/organizations/<org>/settings/apps/new`. A link alone cannot carry it, so the post must come from a web page.
- GitHub then sends the browser to the manifest's `redirect_url` with a one-time `code`. The code is converted at `POST /app-manifests/{code}/conversions`, with no other credential, within an hour. The answer holds the App's `id`, `slug`, `owner`, `permissions` and `events`, its private key (`pem`), and secrets Hyalos does not use.
- Whoever converts the code first gets the key.

On a headless machine the browser is on another device, so neither the page nor the redirect can be on loopback without an SSH tunnel.

## Decision

**A static setup page posts the manifest.** `site/setup` is published with GitHub Pages at `https://hyaloslabs.github.io/hyalos/setup/` (`.github/workflows/pages.yml`), an address short enough to type. It asks for the account to own the App, whether it is an organization, and a name, defaulting to `hyalos-<account>`. It then posts `manifest.json` with that name. The manifest asks for exactly what Hyalos mints tokens from: Contents, Issues and Pull requests read and write, and Metadata read. It sets no events, a private App, the page itself as its `redirect_url`, and Hyalos's site, `https://hyaloslabs.github.io/hyalos/`, as both its homepage and the address of a webhook that is not active. Back on the page, GitHub's `code` is shown for the operator to copy and removed from the address bar. The page loads nothing from elsewhere, and its Content-Security-Policy lets it send forms only to github.com.

**The operator types the code into their own terminal.** `hyalos setup github-app` (and `walk`) asks, when no App is set up, whether to create it from the setup page. On a yes it asks for the owning account, prints the page's address and three short steps, and reads the code as hidden input. The code is a credential: it is never echoed, put in an error or sent anywhere but the conversion. `hyalos` converts it, so the key goes from GitHub to this process only, never through the page, the browser or the driving agent ([ADR-0010](0010-mcp-primary-interface-two-principals.md)).

**What GitHub hands back is checked before anything is written.** The page is not trusted, and anyone can make an App and hand its code to the operator. Four checks:
- The App must be owned by the account the operator named (case-insensitive).
- Its permissions must equal the manifest's exactly, with no events.
- Its key must parse as an RSA key.
- Its webhook, which the conversion does not report, must be absent or at Hyalos's site. This is read with the new key (`GET /app/hook/config`). A webhook elsewhere would tell its owner where the App is installed, even with no events.

Otherwise nothing is written. A code GitHub does not take (404 or 422) writes nothing either.

Then `github-app.pem` and `github.app` (with the App ID, keeping any other lines) are written owner-only. The config directory is checked for that before the code is asked for. Any failure after GitHub has made the App names it and says to delete it, since its key is not kept; that includes an answer that cannot be read.

The answer's other secrets (`client_secret`, `webhook_secret`) are never read into a field. The answer is read into one buffer sized once and zeroed. Copies the TLS transport and the JSON parser make on the way are not zeroed.

Not checked: whether the App is public, and its `setup_url`, `callback_urls` and `request_oauth_on_install`. GitHub's review page shows them before the operator creates the App. A public App can be installed by others only on their own accounts. OAuth on install would need the client secret, which Hyalos never keeps.

**Then it is installed.** `hyalos` prints the App's install link, `github.com/apps/<slug>/installations/new`, and waits for Enter. `hyalos github check <owner/repo>` stays the check that it reaches the repository.

**The way by hand stays.** A no to the setup page, a `<pem>` argument, or a `github.app` naming another `api` takes ADR-0080's path: the browser steps, the App ID and the `.pem` found wherever it was saved. The manifest flow is GitHub.com's, so GitHub Enterprise Server uses that path.

This supersedes ADR-0080's account of `github-app`, for an App not set up yet. Nothing else in ADR-0080 changes.

## Consequences

An operator creates the App without moving a file or setting a field by hand, from a browser on any computer, and the key never sits in a downloads folder. Creating the App becomes a few clicks and one paste.

Hyalos publishes a page, and the page's address is part of setup. If it is down, the operator answers no and makes the App by hand. Because its output is checked, a changed page can fail setup, but it cannot make setup accept an App with other permissions, events, another owner or a webhook elsewhere. A page that kept the code could convert it first and keep the key. The operator's conversion would then fail, nothing would be written, and the operator would be told to delete the App, which also voids the key. The App is never installed in that case.

The setup page's manifest and the check on GitHub's answer must agree. A test pins `manifest.json`'s permissions, events, webhook, homepage and redirect to `app_manifest::permissions()`, `HOMEPAGE` and `SETUP_PAGE`. Adding an action that needs a new permission changes both.

The one-time code is a secret the driving agent must never see, like the key it buys. The set-up-hyalos skill lists it with the others.

The operator's Enter after installing is not checked. An App left uninstalled fails a job with `forge.no_token`, and `hyalos github check` finds it first.

Tests (`setup_ask.rs`):
- The code is converted with no credential, and the App is written owner-only.
- A wider App, another owner, a webhook elsewhere, a refused code and a malformed code each write nothing. Each error after GitHub made the App names it and says to delete it.
- No code, key or client secret is shown.
- A `github.app` naming another API is never offered the setup page.
