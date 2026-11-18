---
name: set-up-hyalos
description: Install Hyalos from its repository and get a first contained job running, with the operator taking every step that needs a secret or their signature. Use when the user asks to set up, install or get started with Hyalos.
---

# Set up Hyalos

You are setting up Hyalos for the person you work for, the **operator**. The goal is a first contained job, started within 10 minutes of beginning. Note the time now, and tell the operator how long it took when the job starts.

`hyalos setup` is your checklist. It lists every step still missing, in order, says who takes each one (`agent` or `operator`), and writes nothing. Work from it rather than from memory. The operator's steps are theirs to take with `hyalos setup walk`, in a terminal of their own, which asks them for everything it needs ([ADR-0080](../../docs/adr/0080-setup-asks-the-operator-in-their-own-terminal.md)).

## Rules

Hyalos has two principals ([ADR-0010](../../docs/adr/0010-mcp-primary-interface-two-principals.md)): you, the driving agent, and the operator. These keep that split true while you set it up:

- **Never read, print, copy or ask for a secret.** That covers the operator's passphrase, a provider API key, the one-time code the setup page shows, and the files `provider.keys`, `github-app.pem`, `operator.key` and `engine.key`. If the operator pastes one to you, tell them to revoke or rotate it. A pasted setup-page code means deleting that App (`github.com/settings/apps`) and running `hyalos setup github-app` again, since whoever exchanges the code first holds the App's key.
- **Never write or summarise a document the operator signs.** They read it and sign it themselves, so what they sign is what they read.
- **Never run a decision.** That means `hyalos approve`, `cancel`, `release`, `merge`, `policy approve`, `agent issue`, `agent revoke` and `run --release-issue`. The operator runs them in **a terminal of their own**, not through you and not with a `!` command here, because what they type and what the command prints would land in this conversation.
- **Never run `hyalos setup` with a subcommand** (`walk`, `operator-key`, `provider-key`, `github-app`, `policy`, `agent`). They ask for secrets and make decisions. Yours are `hyalos setup`, `hyalos setup --json` and `hyalos setup --next`, which only read.
- A step whose `who` is `operator` is theirs. Tell them what to run, then wait until they say it is done.

## How to talk to the operator

The operator works in a second terminal while your output keeps scrolling. Give them **short commands to type**, never a long line or a link to copy: a terminal turns a long line's wrap into spaces or a line break when it is copied, and that broke the provider key's line and the App's link on the first runs.

Keep what you print between steps short, and run long commands in the background, so what they need stays on screen.

## 1. Check the machine

Hyalos runs jobs only on Linux (WSL2 counts). Check for:
- `git`;
- `docker` or `podman`;
- `gh`, logged in (`gh auth status`). A release is downloaded with it, and you open a first issue with it in part 5. Without it, Hyalos is built from source and the operator opens the issue on GitHub.

Then see whether there is a release for this host ([ADR-0081](../../docs/adr/0081-prebuilt-releases.md)): `uname -m` is `x86_64`, and there is a release for this checkout's version: `gh release view "v$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)" --repo hyaloslabs/hyalos` answers. If so, take part 2's release path, which needs no Rust. If not, the source path needs `cargo`, through rustup (the repository pins its toolchain in `rust-toolchain.toml`).

If something is missing, say which and how to install it, then stop until it is there. If rustup installs `cargo` during this session, the operator's terminal does not have it on its `PATH` yet: tell them to open a new terminal, or to run `. ~/.cargo/env` in theirs.

## 2. Install `hyalos` and the sandbox image

**From a release.** Use the release for this checkout's version, so this skill and the binary match. Your shell does not keep variables from one command to the next, so each block below works it out again, and the download goes to a fixed directory. From the repository root, download it and check its SHA-256. Each step runs only if the one before it passed:

```
v=v$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1); d="$HOME/.cache/hyalos/release/$v"
rm -rf "$d" && mkdir -p "$d" && gh release download "$v" --repo hyaloslabs/hyalos -D "$d" -p hyalos-x86_64-linux -p SHA256SUMS && (cd "$d" && sha256sum -c SHA256SUMS)
```

If there is no release for that version, use the source path. If `sha256sum -c` fails, stop and install nothing. Tell the operator the download did not match its checksum, which can mean it was tampered with.

Then check its provenance, where it can be checked. That needs a public repository (`gh repo view hyaloslabs/hyalos --json visibility` says `PUBLIC`) and `gh` 2.68 or later (`gh --version`), which has `--source-ref`. It must have been built by the release workflow from that tag:

```
v=v$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1); d="$HOME/.cache/hyalos/release/$v"
gh attestation verify "$d/hyalos-x86_64-linux" --repo hyaloslabs/hyalos --signer-workflow hyaloslabs/hyalos/.github/workflows/release.yml --source-ref "refs/tags/$v"
```

If it fails, stop and tell the operator, as for a bad checksum. If it cannot be checked (a private repository, or an older `gh`), tell the operator that its provenance was not checked, and why. Then install it:

```
v=v$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1); d="$HOME/.cache/hyalos/release/$v"
install -D -m 755 "$d/hyalos-x86_64-linux" ~/.local/bin/hyalos
```

Run it as `~/.local/bin/hyalos` until `~/.local/bin` is on this shell's `PATH` (`export PATH="$HOME/.local/bin:$PATH"`). Tell the operator to do the same in their terminal. A new terminal is not enough on most desktop distributions, because `~/.profile` adds `~/.local/bin` only if it existed at login. Logging in again makes it last.

Then pull the image. The release names it by digest, and `hyalos setup --json` gives the line, under the `substrate` step's `commands`. Run that `docker pull` in the background. If it is refused because the image is private, the login is the operator's, since the token is theirs. They run these in their own terminal:

```
gh auth refresh -h github.com -s read:packages
gh auth token | docker login ghcr.io -u <their GitHub user> --password-stdin
```

A token of their own with only `read:packages` is narrower than `gh auth token`; they can use one in its place.

Then run the pull again.

**From source.** Both builds take minutes. Start them in the background from the repository root, at the same time, and do not stream their output:

```
cargo install --locked --path core/hyalos
docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .
```

Use `podman` in place of `docker` if that is what the machine has. Keep `--target sandbox`: the Containerfile's last stage is the test image, which is not for jobs.

## 3. The browser work, while it installs

The operator can do this in a browser on any computer, including one other than this machine. Tell them, in these short lines:

1. **A provider key** ([docs/operations/provider-keys.md](../../docs/operations/provider-keys.md)): make a dedicated API key at the provider, set a spend cap on it, and keep it to hand. OpenAI's is for the default harness, Codex. Anthropic's is for Claude Code, and the job then starts with `--harness claude-code`.
2. **The GitHub App needs nothing yet.** `hyalos setup walk` creates it with them later ([ADR-0084](../../docs/adr/0084-setup-creates-the-github-app-from-a-manifest.md)): it sends them to Hyalos's setup page, `hyaloslabs.github.io/hyalos/setup`, in a browser on any computer. GitHub creates the App there with the right permissions, and the page shows a one-time code that they paste into their terminal. That gives Hyalos the App's key, so there is no `.pem` to download or copy. Then they install the App on the repository the first job will work on, from a short link the walk prints.

They keep the key to hand. Nothing is written yet.

**Whenever the operator changes the App's permissions later**, tell them the change does not reach the installation until they accept it there too: on `https://github.com/settings/installations` (for an organization, `https://github.com/organizations/<org>/settings/installations`), they choose **Configure** on the App, then **Review request** in the banner at the top, then **Accept new permissions**. Until then, jobs fail with `forge.no_token`.

## 4. Work through the checklist

Once `hyalos --version` answers, run `hyalos init` (the engine key, which is yours), then:

```
hyalos setup --json
```

It names the config directory (`config_dir`). Then tell the operator, once, to run this in a terminal of their own, in the directory this session runs in:

```
hyalos setup walk
```

It takes each of their steps that is not done, in order, and asks them for what each needs. You do not see their answers, and you do not need to:

| Step | What `walk` asks them for |
| --- | --- |
| `operator_key` | A passphrase of 12 or more characters, twice. It signs their decisions, and you must never see it. |
| `provider_keys` | Which provider, and the key, which it reads without showing. |
| `github_app` | The account to own the App. It sends them to the setup page, takes the code the page shows (not echoed), checks the App GitHub made, and gives the link to install it. For an App made by hand (or on GitHub Enterprise Server) they answer no, and it asks for the App ID and finds its `.pem`. |
| `policy` | It prints the starter policy and asks before they approve it with their passphrase. It allows the first job's actions, with data going only to the model and GitHub, and lets them merge its pull request after they review it. |
| `agent_credential` | That this is the directory they run Claude Code in. It issues a credential and registers this client's server with it, without showing it. It adds `--harness claude-code` when their only key is Anthropic's. |

Each step is also its own command (`hyalos setup provider-key`, `github-app`, `policy`, `agent`), safe to run again, which asks before it replaces anything. If a step fails, `walk` says what was not written; once they fix it, they run `hyalos setup walk` again.

Meanwhile, the `substrate` step is yours: wait for the image's pull or build. If it is still not ready, `hyalos doctor` says why. `walk` does not wait for it.

When they say `walk` is done, run `hyalos setup` until it prints `setup: 7 of 7 steps done`. Then check that the server was registered for the directory this session runs in:

```
claude mcp list
```

Look for a line starting `hyalos:`. Never run `claude mcp get hyalos`: it shows the server's details, which can include the credential. If there is no `hyalos:` line, they ran `walk` elsewhere. Ask them to run `hyalos setup agent` in this session's directory and answer yes to replacing the credential, then run `claude mcp list` again. The MCP tools appear in your next session, and the first job does not wait for them.

## 5. The first job

1. Ask the operator for the repository (`owner/name`, the one the App is installed on) and the number of a small open issue there: one change a short job can make, such as a typo or a one-line fix. Not an epic or a tracking issue (one with sub-issues or a task list), which a single job cannot finish. If the repository has no such issue, offer to open one for a small change they choose, such as a line added to the README. Only on their yes, and only if `gh` is logged in, run `gh issue create --repo <owner/name> --title '<title>' --body '<body>'`, worded from what they asked for. Otherwise they open it on GitHub.
2. Run `hyalos github check <owner/name>`. It must exit 0. It tries each permission a job needs, and if one is refused it names it: the operator fixes it on the App's settings page and then accepts the change on the installation's page, which GitHub asks for separately.
3. Start the job:

   ```
   hyalos start <owner/name> <issue-number>
   ```

   Add `--harness claude-code` if their key is Anthropic's. This command is the CLI twin of the `start_job` tool ([ADR-0075](../../docs/adr/0075-the-thin-cli-for-the-agents-tools.md)), and it prints the task's id.
4. Tell the operator how long setup took, from the time you noted at the start to now.
5. Check `hyalos status <task>` about every 30 seconds. After triage and planning, the status says the actor waits for the operator to release the issue ([ADR-0078](../../docs/adr/0078-agent-started-runs-wait-for-the-operator-to-release-the-issue.md)). Tell the operator once that this is the step where they let the job read the issue's text: they read the issue on GitHub, then run this in their own terminal:

   ```
   hyalos release <task>
   ```

   You do not run it. If the status says the issue was released by the operator's policy, nothing waits: the operator's policy permits `release_issue`, and everyone who wrote the issue can write to the repository ([ADR-0103](../../docs/adr/0103-policy-releases-an-issue-written-only-by-the-repository-s-writers.md)). Tell the operator which kind of release happened. Keep checking until the run is over, then show `hyalos result <task>`. A run that succeeds has opened a pull request on the branch `hyalos/job-<id>`. Waiting for the release counts against the run's 30 minutes.
6. If the run is held, `hyalos approvals` shows what is waiting and the command the operator runs to decide it. You do not decide it.
7. Once the run has opened its pull request, find it and its head commit:

   ```
   gh pr list --repo <owner/name> --state open --json number,url,headRefName,headRefOid,createdAt --jq '.[] | select(.headRefName | startswith("hyalos/job-"))'
   ```

   The branch is named after the job, whose id is not the task's, so match the prefix and take the one whose `createdAt` is this run's. Tell the operator that merging is theirs and comes after review: they read the pull request on GitHub, then, if they want it, run this in their own terminal:

   ```
   hyalos merge <owner/name> <number> --head <headRefOid>
   ```

   You do not run it. If it fails with `no manifest: policy does not allow merge`, their policy is older than the starter that permits it: they run `hyalos setup policy` in their terminal, answer yes to both of its questions (replacing the policy in force, then approving the starter), give their passphrase, and run the merge again. For any other refusal, [approvals.md](../../docs/operations/approvals.md#merging) says what each code means. `merge.not_mergeable` is a conflict, checks still running, a closed pull request, or the repository's settings: if checks are running, the operator runs it again once they pass. `forge.upstream_refused` is the repository's settings, such as required reviews or allowed merge methods ([github-app.md](../../docs/operations/github-app.md), section 6).

After this, in a new session with the MCP server, use the tools `start_job`, `job_status`, `job_result`, `list_approvals` and `list_learnings` in place of the commands.
