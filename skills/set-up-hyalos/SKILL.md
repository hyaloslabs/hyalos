---
name: set-up-hyalos
description: Install Hyalos from its repository and get a first contained job running, with the operator taking every step that needs a secret or their signature. Use when the user asks to set up, install or get started with Hyalos.
---

# Set up Hyalos

You are setting up Hyalos for the person you work for, the **operator**. The goal is a first contained job, started within 10 minutes of beginning. Note the time now, and tell the operator how long it took when the job starts.

`hyalos setup` is your checklist. It lists every step still missing, in order, says who takes each one (`agent` or `operator`), gives each step's commands, and writes nothing. Work from it rather than from memory.

## Rules

Hyalos has two principals ([ADR-0010](../../docs/adr/0010-mcp-primary-interface-two-principals.md)): you, the driving agent, and the operator. These keep that split true while you set it up:

- **Never read, print, copy or ask for a secret.** That covers the operator's passphrase, a provider API key, and the files `provider.keys`, `github-app.pem`, `operator.key` and `engine.key`. If the operator pastes one to you, tell them to revoke or rotate it.
- **Never write or summarise a document the operator signs.** They make it and read it themselves, so what they sign is what they read.
- **Never run a decision.** That means `hyalos approve`, `cancel`, `merge`, `policy approve`, `agent issue`, `agent revoke` and `run --release-issue`. The operator runs them in **a terminal of their own**, not through you and not with a `!` command here, because what they type and what the command prints would land in this conversation.
- A step whose `who` is `operator` is theirs. Tell them what to do, then wait until they say it is done.

## How to hand the operator a step

The operator works in a second terminal and copies from this one, while your output keeps scrolling. Make that easy (#258):

- **One step at a time.** Give only the step they take now. Do not list the steps after it. Wait until they say it is done, check it, then give the next.
- **One block to copy.** Put a one-line note on what the step does, then a single fenced block holding exactly the commands to paste, and nothing after the block. Take the commands from the step's `commands` in `hyalos setup --json`, filling in any `<...>` you know (such as the App ID) and leaving the rest for them.
- **Point them at `hyalos setup --next`.** Once `hyalos` is installed, tell them once that it prints the next step's commands in their own terminal, so they need not copy from this window at all.
- Keep what you print between steps short, and run long commands in the background, so the step they are on stays on screen.

## 1. Check the machine

Hyalos runs jobs only on Linux (WSL2 counts). Check for:
- `git`;
- `cargo`, through rustup (the repository pins its toolchain in `rust-toolchain.toml`);
- `docker` or `podman`.

If one is missing, say which and give the install as one block to copy, then stop until it is there.

## 2. Start the two long builds

Both take minutes. Start them in the background from the repository root, at the same time, and do not stream their output:

```
cargo install --locked --path core/hyalos
docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .
```

Use `podman` in place of `docker` if that is what the machine has. Keep `--target sandbox`: the Containerfile's last stage is the test image, which is not for jobs.

## 3. The operator's first two steps, while the builds run

These need no Hyalos binary. Give them one at a time.

**First, the GitHub App** ([docs/operations/github-app.md](../../docs/operations/github-app.md)). Ask for their GitHub user name, then give them this link to open, with `<name>` filled in:

```
https://github.com/settings/apps/new?name=hyalos-<name>&url=https://github.com/<name>&public=false&webhook_active=false&contents=write&issues=write&pull_requests=write&metadata=read
```

For an organization's repositories it starts `https://github.com/organizations/<org>/settings/apps/new?` with the same parameters. On that page they:
1. check Contents, Issues and Pull requests are Read and write, Metadata Read-only, everything else No access;
2. choose **Create GitHub App** and note the App ID;
3. choose **Generate a private key**, which downloads one `.pem` file (on WSL2 under `/mnt/c/Users/<user>/Downloads`);
4. choose **Install App** on the repository the first job will work on, with "Only select repositories".

They keep the App ID and the file; the files are written in part 4.

**Then, a provider key** ([docs/operations/provider-keys.md](../../docs/operations/provider-keys.md)). They make a dedicated API key at the provider and set a spend cap on it, and keep the key to hand. OpenAI's is for the default harness, Codex; Anthropic's is for Claude Code, and the job then starts with `--harness claude-code`. The key is written in part 4.

## 4. Work through the checklist

Once `cargo install` has finished and `hyalos --version` answers, run:

```
hyalos setup --json
```

It names the config directory (`config_dir`); use that path, not a guess. Take the first step that is not done:

| Step | Who | What happens |
| --- | --- | --- |
| `engine_key` | agent | Run `hyalos init`. |
| `operator_key` | operator | They run `hyalos operator init` and choose a passphrase of 12 or more characters. It signs their decisions, and you must never see it. |
| `substrate` | agent | Wait for the image build. If it is still not ready, `hyalos doctor` says why. |
| `provider_keys` | operator | The commands read the key from their terminal without showing it and write `provider.keys` at mode 0600. They write `openai.key`; for an Anthropic key the operator changes it to `anthropic.key`. |
| `github_app` | operator | The commands move the downloaded `.pem` into place and write `github.app`. Fill in the App ID and the download's path as they tell you them. The detail says what is still wrong, such as a file with the wrong mode. |
| `policy` | operator | They print the starter, read it, and approve it. It allows the first job's actions, with data going only to the model and GitHub. You do not write or edit the file they approve. |
| `agent_credential` | operator | They issue a credential and register this client's server in one line, run in the repository's directory, so the credential goes straight to the client without being shown. They add `--harness claude-code` at the end if their only key is Anthropic's (the server's jobs default to Codex). The MCP tools appear in your next session, and the first job does not wait for them. |

Hand each operator step over as in "How to hand the operator a step". Run `hyalos setup` again after each step, until it prints `setup: 7 of 7 steps done`.

## 5. The first job

1. Ask the operator for the repository (`owner/name`, the one the App is installed on) and the number of an open issue there.
2. Run `hyalos github check <owner/name>`. It must exit 0. It tries each permission a job needs, and if one is refused it names it: the operator fixes it on the App's settings page and then accepts the change on the installation's page, which GitHub asks for separately.
3. Start the job:

   ```
   hyalos start <owner/name> <issue-number>
   ```

   Add `--harness claude-code` if their key is Anthropic's. This command is the CLI twin of the `start_job` tool ([ADR-0075](../../docs/adr/0075-the-thin-cli-for-the-agents-tools.md)), and it prints the task's id.
4. Tell the operator how long setup took, from the time you noted at the start to now.
5. Check `hyalos status <task>` about every 30 seconds until the run is over, then show `hyalos result <task>`. A run that succeeds has opened a pull request on the branch `hyalos/job-<id>`.
6. If the run is held, `hyalos approvals` shows what is waiting and the command the operator runs to decide it. You do not decide it.

After this, in a new session with the MCP server, use the tools `start_job`, `job_status`, `job_result` and `list_approvals` in place of the commands.
