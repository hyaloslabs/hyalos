---
name: set-up-hyalos
description: Install Hyalos from its repository and get a first contained job running, with the operator taking every step that needs a secret or their signature. Use when the user asks to set up, install or get started with Hyalos.
---

# Set up Hyalos

You are setting up Hyalos for the person you work for, the **operator**. The goal is a first contained job, started within 10 minutes of beginning. Note the time now, and tell the operator how long it took when the job starts.

`hyalos setup` is your checklist. It lists every step still missing, in order, says who takes each one (`agent` or `operator`), and writes nothing. Work from it rather than from memory.

## Rules

Hyalos has two principals ([ADR-0010](../../docs/adr/0010-mcp-primary-interface-two-principals.md)): you, the driving agent, and the operator. These keep that split true while you set it up:

- **Never read, print, copy or ask for a secret.** That covers the operator's passphrase, a provider API key, and the files `provider.keys`, `github-app.pem`, `operator.key` and `engine.key`. If the operator pastes one to you, tell them to revoke or rotate it.
- **Never write or summarise a document the operator signs.** They make it and read it themselves, so what they sign is what they read.
- **Never run a decision.** That means `hyalos approve`, `cancel`, `merge`, `policy approve`, `agent issue`, `agent revoke` and `run --release-issue`. The operator runs them in **a terminal of their own**, not through you and not with a `!` command here, because what they type and what the command prints would land in this conversation.
- A step whose `who` is `operator` is theirs. Tell them exactly what to do, then wait until they say it is done.

## 1. Check the machine

Hyalos runs jobs only on Linux (WSL2 counts). Check for:
- `git`;
- `cargo`, through rustup (the repository pins its toolchain in `rust-toolchain.toml`);
- `docker` or `podman`.

If one is missing, say which and how to install it, and stop until it is there.

## 2. Start the two long builds, and the operator's first steps

Both builds take minutes. Start them in the background from the repository root, at the same time:

```
cargo install --locked --path core/hyalos
docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .
```

Use `podman` in place of `docker` if that is what the machine has. Keep `--target sandbox`: the Containerfile's last stage is the test image, which is not for jobs.

While they build, ask the operator to do the two things that need no Hyalos binary:

- **The GitHub App** ([docs/operations/github-app.md](../../docs/operations/github-app.md)). Give them this prefilled link, with `<name>` replaced by their GitHub user name:

  ```
  https://github.com/settings/apps/new?name=hyalos-<name>&url=https://github.com/<name>&public=false&webhook_active=false&contents=write&issues=write&pull_requests=write&metadata=read
  ```

  For an organization's repositories, use `https://github.com/organizations/<org>/settings/apps/new?` with the same parameters.

  On that page they should:
  1. Check the permissions shown: Contents, Issues and Pull requests are Read and write, Metadata is Read-only, and everything else is No access.
  2. Choose **Create GitHub App** and note the App ID.
  3. Generate a private key. The browser downloads one `.pem` file. They move that file to `github-app.pem` in the config directory and `chmod 600` it. The config directory is `$HYALOS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/hyalos`, else `~/.config/hyalos`; once Hyalos is installed, `hyalos setup` prints it. Create it with `mkdir -p -m 700` if it does not exist. On WSL2 the download is under `/mnt/c/Users/<user>/Downloads`.
  4. Write `app_id = <id>` into `github.app` in the config directory.
  5. **Install App** on the repository the first job will work on, with "Only select repositories".

  You may write `github.app` yourself once they tell you the id, since it holds no secret.

- **A provider key** ([docs/operations/provider-keys.md](../../docs/operations/provider-keys.md)). Ask them to make a dedicated API key and set a spend cap on it at the provider. They then write it into `provider.keys` in the config directory themselves and `chmod 600` it. The file holds one line:
  - `openai.key = ...` for the default harness, Codex;
  - or `anthropic.key = ...` for Claude Code, in which case start the job with `--harness claude-code`.

## 3. Work through the checklist

Once `cargo install` has finished and `hyalos --version` answers, run:

```
hyalos setup
```

Add `--json` if you would rather parse it. Its second line names the config directory; use that path, not a guess. Take the first step that is not done:

| Step | Who | What happens |
| --- | --- | --- |
| `engine_key` | agent | Run `hyalos init`. |
| `operator_key` | operator | They run `hyalos operator init` in their own terminal and choose a passphrase of 12 or more characters. It signs their decisions, and you must never see it. |
| `substrate` | agent | Wait for the image build. If it is still not ready, `hyalos doctor` says why. |
| `provider_keys`, `github_app` | operator | The steps they started in part 2. The detail says what is still wrong, such as a file with the wrong mode. |
| `policy` | operator | In their own terminal they run `hyalos policy starter > policy.cedar`, read it, then run `hyalos policy approve policy.cedar`. The starter allows the first job's actions, with data going only to the model and GitHub. You do not write or edit the file they approve. |
| `agent_credential` | operator | They run `hyalos agent issue claude-code` in their own terminal, then give this client the server with `claude mcp add hyalos -e HYALOS_AGENT_CREDENTIAL=<credential> -- hyalos mcp`, adding `--harness claude-code` at the end if their only key is Anthropic's (the server's jobs default to Codex). The credential is printed once. The MCP tools appear in your next session, and the first job does not wait for them. |

Run `hyalos setup` again after each step, until it prints `setup: 7 of 7 steps done`.

## 4. The first job

1. Ask the operator for the repository (`owner/name`, the one the App is installed on) and the number of an open issue there.
2. Run `hyalos github check <owner/name>`. It must exit 0. If it does not, the message says which step to go back to.
3. Start the job:

   ```
   hyalos start <owner/name> <issue-number>
   ```

   Add `--harness claude-code` if their key is Anthropic's. This command is the CLI twin of the `start_job` tool ([ADR-0075](../../docs/adr/0075-the-thin-cli-for-the-agents-tools.md)), and it prints the task's id.
4. Tell the operator how long setup took, from the time you noted at the start to now.
5. Check `hyalos status <task>` about every 30 seconds until the run is over, then show `hyalos result <task>`. A run that succeeds has opened a pull request on the branch `hyalos/job-<id>`.
6. If the run is held, `hyalos approvals` shows what is waiting and the command the operator runs to decide it. You do not decide it.

After this, in a new session with the MCP server, use the tools `start_job`, `job_status`, `job_result` and `list_approvals` in place of the commands.
