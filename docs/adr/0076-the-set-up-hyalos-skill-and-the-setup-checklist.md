# ADR-0076: The set-up-hyalos skill and the setup checklist

**Status:** Superseded by [ADR-0077](0077-handing-the-operator-one-step-at-a-time.md)

## Context

The PRD's first goal is a single install that reaches a first contained job in under 10 minutes, and S5.5 (#33) has the user tell their agent "set up Hyalos". The setup was a page of procedures across six documents. Several of its steps cannot be the agent's:
- the operator's passphrase and the decisions it signs ([ADR-0070](0070-the-operators-signature-on-every-human-decision.md)): approving a policy, issuing the agent's credential;
- the secrets: a provider key ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)) and the GitHub App's private key ([ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md)).

An agent that helps set up Hyalos has a shell as the operator's user. That is already past the credential's boundary ([ADR-0075](0075-the-thin-cli-for-the-agents-tools.md)). The passphrase is the boundary that remains, and it holds only if the agent never sees it. Releases and an install source are not decided yet (#81, M3), so the install is built from the repository.

## Decision

**`hyalos setup [--image <image>] [--json]` is the checklist.** It lists the seven steps to a first job, in order:
1. `engine_key`
2. `operator_key`
3. `substrate`
4. `provider_keys`
5. `github_app`
6. `policy`
7. `agent_credential`

For each it gives whether it is done, what it found or what is wrong, what to do, and the procedure's document.

It checks each step the way a job would read it:
- the engine key with `keys::load`, its public half against `engine.pub`, and the log against that;
- the operator key with `operator::check`: the file is the operator's alone, parses and is sealed to `operator.pub`, without the passphrase;
- the substrate with `detect_with`, on a thread with `hyalos run`'s 60-second timeout ([ADR-0050](0050-running-one-job-from-the-command-line.md));
- `provider.keys` with the broker's own reader;
- the App with `GitHubApp::load`;
- the policy in force from the log, which must still validate, since a stored policy that does not holds every job ([ADR-0034](0034-the-operator-policy-store-and-minting-within-policy.md)), and the agent credentials in force.

It writes nothing but what substrate detection caches, as `hyalos doctor` does, and makes no network call. It names the config directory, so the operator's files go where Hyalos reads them. Whether the App is installed on a repository is `hyalos github check`. It prints no secret: the details name files and lines, never contents. It exits 0 when every step is done, 1 when one is not, and 2 for bad usage. It is a setup command, so it has no MCP tool (ADR-0075).

**Each step names who takes it.** The agent takes setup a shell can do with nothing secret and nothing decided: `hyalos init` and building the image (`--target sandbox`; the Containerfile's last stage is the test image, with curl and the test CA). The operator takes every step that holds a secret or is a signed decision:
- `operator init`;
- `provider.keys`;
- the App and its key;
- the policy: the operator prints the starter, reads it and approves it, so the file they sign is one the agent never wrote;
- `agent issue`, and registering the server with the client (with `--harness claude-code` when their only key is Anthropic's, since the server's template defaults to Codex).

The operator runs those in a terminal of their own, because what they type into the agent's session, or what a command prints there, reaches the agent.

**`hyalos policy starter` prints a starter policy.** It permits `read`, `push`, `open_pr` and `data_flow`, and forbids a tainted job's `job->web`, which is what the fix-issue workflow needs and no more. It is compiled in from `core/hyalos/policies/starter.cedar`. Nothing approves it by itself: the operator reads it and approves it. A test runs the fix-issue workflow under it.

**The skill is a plain Agent Skill in the repository.** It lives in `skills/set-up-hyalos/SKILL.md`, in the agentskills.io format, with `.claude/skills/set-up-hyalos` linking to it so Claude Code finds it in a clone. It:
- checks the machine;
- starts `cargo install --locked --path core/hyalos` and the image build together, and has the operator create the App while they run (from a link prefilled with its permissions) and write the provider key;
- loops on `hyalos setup`;
- starts the first job with `hyalos start`, so that job does not wait for the client to load the new MCP server.

It states the rules: never read or ask for a secret, never run a decision or write a document the operator signs, and send the operator to their own terminal for those.

## Consequences

A new user can say "set up Hyalos" in a clone and be walked to a first job, with every secret and every decision staying theirs. The checklist is code, so it follows the files and the log rather than drifting from the documents. The time to a first job is measured by a live run on a clean machine. Building from source spends most of it, so the 10-minute target leans on the builds running alongside the operator's steps, until releases exist (#81). A skill cannot enforce its rules: an agent with a shell can read the secret files. The passphrase stays out of its reach, and so do the decisions it signs.
