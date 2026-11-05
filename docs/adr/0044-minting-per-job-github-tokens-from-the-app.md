# ADR-0044: Minting per-job GitHub tokens from the App

**Status:** Accepted. [ADR-0045](0045-forge-tokens-up-to-4-kib.md) replaces the token's 255-character cap with 4,096 (GitHub's tokens are now about 400) and revokes any issued token the engine refuses. [ADR-0050](0050-running-one-job-from-the-command-line.md) adds `git`, the git host, to `github.app` (required with any `api` but api.github.com), and `hyalos run` mints and revokes the token for each job.

## Context

[ADR-0004](0004-one-signed-manifest-per-job.md) decides that the engine holds the GitHub App private key and mints a per-job token. The token is scoped to the manifest's repository and to the forge permissions its actions need, handed to the broker with the manifest, and expires with the job. No operator PAT exists, and a broker compromise exposes the tokens of live jobs, never the App key. It leaves the mechanism open:
- where the key and the App's settings live;
- how the token is minted and how its scope is checked;
- which permission each action needs;
- how the token reaches the broker;
- what "expires with the job" means when GitHub fixes an installation token's life at one hour.

These are stories S3.9 (#69) and S3.4 (#19). GitLab is not built here.

## Decision

**Files.** The App's settings are `github.app` in the config directory. It holds one `name = value` per line: `app_id` (required) and `api` (optional; default `https://api.github.com`, or a GitHub Enterprise Server's `https://host/api/v3`). Any other name is refused by line number. A name containing `token`, or `pat`, is refused with a message saying Hyalos never takes a personal access token. The App's private key is `github-app.pem` beside it, exactly as GitHub generates it (PKCS#1; PKCS#8 also read). It is read with `keys::read_private`, so a key file that is not a regular file owned by the engine's user and closed to group and others is refused ([ADR-0030](0030-hardening-the-engines-key-owner-and-core-dumps.md)). The engine reads both; the broker reads neither.

**Code.** `hyalos::github` in the `hyalos` crate, which runs in the engine process. The engine crate cannot do I/O ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)). The JWT is RS256, signed with `ring`, which rustls already brings into the build. The `rsa` crate is not used because of its unfixed timing advisory (RUSTSEC-2023-0071). The PEM is read with `rustls-pki-types`. Requests go through the broker crate's `HttpTransport`, which gains `request(method, ..)` for `GET` and `DELETE`. It keeps TLS to the compiled-in roots ([ADR-0039](0039-tls-to-model-providers.md)), and plain HTTP only to loopback, which the tests use.

**Minting.** `GitHubApp::mint(manifest, now)`:
1. Works out the permissions from the manifest's actions (table below). If none are needed, it returns no token and calls GitHub not at all, so a job that does not touch the forge holds no token.
2. Signs a JWT as the App (`iss` the App id, `iat` a minute back for clock drift, `exp` nine minutes ahead, inside GitHub's ten).
3. Finds the App's installation on the repository (`GET /repos/{owner}/{repo}/installation`). A 404 says the App is not installed there.
4. Asks for an installation token with `repositories: [repo]` and exactly those permissions (`POST /app/installations/{id}/access_tokens`).
5. Checks what GitHub says it granted: repository selection `selected`, only this repository, and each permission no higher than asked, apart from GitHub's own `metadata: read`. A wider grant is revoked at once and refused (`MintError::Wider`), never used.

The repository must be `owner/name` with the characters GitHub allows, so it is safe in a path. Errors give GitHub's status, never its body, a token or the key.

**The permission table.** A manifest cannot reach a permission this table does not name:

| Action | Permissions |
|---|---|
| `read` | contents, issues, pull requests: read |
| `push` | contents: write |
| `open_pr` | pull requests: write |
| `comment` | issues: write |
| `merge` | contents, pull requests: write |
| `use_tool`, `data_flow`, `request_host`, `spend_over_cap` | none |
| `publish`, `change_policy`, `export_credentials`, and any other name | refused: no token is minted |

The higher level wins where actions overlap. No action grants `workflows`, `administration` or any organization permission, so a job's token cannot change CI, settings or members. `merge` is mapped, but no manifest with it is signed until approvals exist (S5.4, [ADR-0034](0034-the-operator-policy-store-and-minting-within-policy.md)).

**Handing it over.** `ToBroker::StartJob` gains `forge_token`, a `hyalos_types::ForgeToken`: the token (1 to 255 printable ASCII characters, no space) and when GitHub stops accepting it. Its `Debug` form hides the token. The field is absent when the job needs none. This is wire version 3. The token crosses only the private channel ([ADR-0035](0035-the-private-channel-between-engine-and-broker.md)) and is the only secret the engine sends. The broker keeps it for an admitted job (`Broker::forge_token`), never for a refused one, and drops it at `end_job`. Using it is the git proxy (S3.3, #18) and the forge API (S3.12, #79).

**Expiry.** GitHub fixes an installation token's life at one hour. The engine revokes it when the job ends (`GitHubApp::revoke`, `DELETE /installation/token`), so it stops working with the job and not only an hour later. A job that outlives its token loses forge access when it expires; renewing a token mid-job is not built. The token is never written to the log, and nothing records which permissions a job's token had. The job event's credential tier is the end-to-end job (#70).

**Checking the setup.** `hyalos github check <owner/repo>` mints a `contents: read` token for the repository. It prints the installation and the expiry, never the token, and revokes it. It exits 0 when the App can mint, 1 when it cannot (saying why), and 2 for bad usage. The procedure for creating and installing the App is `docs/operations/github-app.md`.

## Consequences

A job's forge authority is now a GitHub token for one repository with the permissions its manifest's actions need, and it ends with the job. The App key never leaves the engine process; the broker is started with no argument or file that names it, and what crosses to it holds the token alone.

The engine calls GitHub twice to start a forge-touching job, and needs network access to it. A GitHub outage refuses those jobs, which fails closed. A revoke that fails leaves the token alive until GitHub expires it, at most an hour; the engine reports it.

The permission table is the interface between manifest actions and forge authority. Widening it, by adding a permission or giving an action more, needs a new ADR, as a manifest field that widens authority does (ADR-0004). The tests use a key made for them alone (`core/hyalos/tests/fixtures/github-app-test.pem`), which is no App's key.
