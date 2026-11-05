# The GitHub App: creating and installing it

How to give Hyalos access to a repository. Hyalos never uses a personal access token. The engine holds a GitHub App's private key and, for each job, mints a token for one repository with only the permissions that job's actions need. The broker gets that token, never the key, and the engine revokes it when the job ends. The reasons are in [ADR-0004](../adr/0004-one-signed-manifest-per-job.md) and [ADR-0044](../adr/0044-minting-per-job-github-tokens-from-the-app.md). This page is the procedure.

## 1. Create the App

On GitHub, go to your account's or organization's **Settings → Developer settings → GitHub Apps → New GitHub App**. Use an organization if the repositories belong to one.

- **Name**: anything, such as `hyalos-<your-name>`.
- **Homepage URL**: any URL you control. It is not used.
- **Webhook**: untick **Active**. Hyalos does not receive webhooks.
- **Repository permissions**: grant only these:
  - **Contents**: Read and write
  - **Issues**: Read and write
  - **Pull requests**: Read and write
  - **Metadata**: Read-only (GitHub requires it)

  Leave everything else at **No access**, including Workflows, Administration and every organization and account permission. A job's token can only narrow what the App has, never widen it, and no job is ever given those.
- **Where can this GitHub App be installed?**: **Only on this account**.

Create it. Note the **App ID** on its settings page.

## 2. Generate its private key

On the App's settings page, under **Private keys**, choose **Generate a private key**. Your browser downloads a `.pem` file. It is the one long-lived secret here. Move it into the Hyalos config directory (`$HYALOS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/hyalos`, else `~/.config/hyalos`) and make it yours alone:

```
mv ~/Downloads/<app-name>.*.private-key.pem ~/.config/hyalos/github-app.pem
chmod 600 ~/.config/hyalos/github-app.pem
```

Hyalos refuses a key file that belongs to another user, can be read by group or others, or is not a regular file. Keep a copy where a compromise of this machine cannot reach, as for `engine.key` ([keys and recovery](keys-and-recovery.md)), or plan to generate a new one if this one is lost.

## 3. Write the settings

Create `~/.config/hyalos/github.app`:

```
# The App's number, from its settings page.
app_id = 123456

# Optional: GitHub Enterprise Server's API and git host. Leave both out for github.com.
# Jobs clone and push through the broker to the git host, so with any other api it is required.
# api = https://github.example.com/api/v3
# git = https://github.example.com
```

It holds no secret. A line that names a token is refused: there is no setting for one.

## 4. Install it on a repository

On the App's settings page, choose **Install App**, pick the account, and choose **Only select repositories**. Select the repositories Hyalos may work on. Hyalos mints tokens only for repositories the App is installed on, and each token covers one repository.

## 5. Check it

```
hyalos github check owner/repo
```

It mints a read-only token for the repository, shows the installation and when the token would expire, and revokes it at once. It never prints the token. It exits 0 when the App can mint for that repository. If it fails, the message says which step:
- **not installed on owner/repo**: go back to step 4.
- **status 401**: `app_id` does not match the key, the key was replaced on GitHub, or this machine's clock is off by more than a minute.

## Rotating or revoking

To rotate the key, generate a new one on the App's settings page, replace `github-app.pem`, run `hyalos github check`, then delete the old key on GitHub. To stop Hyalos reaching a repository, remove it from the installation. To stop it reaching any, suspend or uninstall the App. Tokens already minted stop working within an hour at most, and Hyalos revokes each one when its job ends.
