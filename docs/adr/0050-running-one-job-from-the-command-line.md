# ADR-0050: Running one job from the command line

**Status:** Proposed

## Context

S3.10 (#70) asks for a cut-down reference job: given the text of an issue on the command line, a real harness in the sandbox pushes a branch and opens a pull request. The pieces exist. Nothing joins them, because no command starts the engine:
- the engine signs a manifest only within policy ([ADR-0034](0034-the-operator-policy-store-and-minting-within-policy.md));
- the broker admits a job and serves its socket: models, egress, git and pull requests ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md), [ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md), [ADR-0046](0046-the-git-proxy.md), [ADR-0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md));
- the substrates can be detected ([ADR-0043](0043-choosing-the-substrate-recording-it-and-doctor.md));
- the GitHub App mints a token for one repository ([ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md));
- the image has the harnesses and git ([ADR-0040](0040-the-sandbox-image.md), [ADR-0049](0049-git-in-the-sandbox-image.md)).

The issue also asks:
- the job's event records the substrate and the credential tier;
- the engine records every gate answer the broker reports, including egress refusals sent unprompted;
- detection refuses an unsupported host or one with no ready substrate, and has a timeout;
- the token is revoked at the end, and a failed revoke is reported;
- no provider key or forge token is in the sandbox's environment, files or memory.

Three gaps stand in the way:
- **The broker reaches github.com only.** A test cannot point it at a stand-in, and an operator on GitHub Enterprise Server cannot point it at their host.
- **The engine cannot wait on the broker.** `BrokerProcess::recv` reads with a 30-second timeout and closes the channel when it passes. A job is quiet for minutes, and the engine must also watch the sandbox.
- **There is no way to approve a policy from the command line.** With no operator policy the baseline allows nothing, so no manifest is ever signed.

## Decision

**`hyalos run --repo <owner/name> (--issue <text> | --issue-file <file>)` runs one job.**

Options:
- `--base` (default `main`);
- `--harness codex|claude-code` (default `codex`);
- `--image` (default `hyalos-sandbox:latest`);
- `--budget-tokens` (default 200,000);
- `--minutes` (default 30);
- `--allow-host` (repeatable).

It runs in this order. Everything that can refuse without a job comes before anything is written.

1. **Find a substrate.** `hyalos_substrate::detect` runs on its own thread, with a timeout of 60 seconds.
   - A host that is not Linux is refused with `Unsupported`'s message.
   - With no ready substrate, the refusal includes what `hyalos doctor` would print.
   - If a runtime hangs past the timeout, the job is refused rather than hung; the thread is left to finish on its own.
   - `Substrate` and `Sandbox` are now `Send`, so a driver can cross threads.
2. **Check the configuration.** The GitHub App and `provider.keys` must be in place, and the engine's key must load.
3. **Record the task and the job.** `task.created` records a SHA-256 of the repository, base and issue text. `job.started` records the chosen substrate and the credential tier.
4. **Sign the manifest.** The manifest is for an actor, so it is tainted ([ADR-0005](0005-provenance-is-a-type.md)). It holds:
   - `read`, `push` and `open_pr`, which are all routine;
   - `branch_prefix` set to `hyalos/job-<id>`. That is a branch name, not a directory, so the job may push that one branch;
   - `pr_base` set to `--base`;
   - the hosts given with `--allow-host`;
   - the budget, and a deadline `--minutes` from now.

   The issue text is the operator's, so the step reads `Provenance::Operator`. If the policy refuses, the job ends `failed`.
5. **Mint the token.** `GitHubApp::mint` issues a token for that one repository. From here, every way the job can end revokes the token.
6. **Start the broker and the job's socket.** `hyalos broker` starts with `provider.keys` and the App's forge addresses. `start_job` carries the token. Anything but `admitted` ends the job `failed`.
7. **Run the sandbox.** The harness command is `hyalos-forward codex exec --skip-git-repo-check <prompt>`, or `hyalos-forward claude -p <prompt> --tools Bash --allowedTools Bash`.
   - The environment carries `HYALOS_REPO`, `HYALOS_BRANCH`, `HYALOS_BASE` and `HYALOS_REMOTE` (`http://127.0.0.1:8089/git/<owner>/<name>.git`).
   - The prompt gives the issue text and the steps: clone, branch, commit, push, then `hyalos-pr`.
   - The sandbox's deadline is the manifest's.
8. **Record what the broker reports while the sandbox runs.** The engine polls the broker's channel and records:
   - `gate` with `allow` or `deny`, through `Engine::submit` as `Origin::Broker`, as `gate.answered`;
   - `gate` with `unknown`, through `Engine::rule_gate`. Today's broker never sends one. If it does, the job ends held with irreversible stakes, which only a human releases;
   - `refused`, which marks the job failed.

   When the sandbox ends, the engine reads what is left on the channel, then sends `end_job` and `shutdown`.
9. **Revoke the token and record the end.** `job.ended` is:
   - `succeeded` if the harness exited 0 and nothing was refused;
   - `deadline_exceeded` if the deadline passed;
   - `failed` otherwise.

   A held job already has its end and hold from `rule_gate`.
   - A failed revoke is printed in capitals with what to do, and the command exits 1 even if the job succeeded. No event records it: the token expires within the hour, and the log records decisions, not housekeeping.

**`job.started` gains `credentials`, a `CredentialTier` (schema version 7).**
- `broker_held` is the one tier v1 has ([ADR-0016](0016-agent-runtime-and-broker-protocols.md)): the broker holds every credential and the sandbox holds none.
- `unrecorded` is what an older event reads as, as with `substrate`.
- There is no value for a tier that puts a credential in the sandbox. Such a tier needs its own decision, and with it a value that a reader of the log can tell apart.

**Wire version 5: `hello` gains an optional `forge`, `{api, git}`.**
- The broker uses them for the git proxy and the forge, and falls back to github.com when they are absent.
- The broker's transport still refuses plain `http` anywhere but loopback ([ADR-0039](0039-tls-to-model-providers.md)), whatever the engine sends.
- `github.app` gains `git`, the git host. It defaults to `https://github.com` when `api` is the default; with any other `api` it is required, since the git host cannot be guessed from it.
- `GitHubApp::forge_bases` passes both to the broker.

**`BrokerProcess` reads the channel on a thread of its own after the handshake.**
- `poll(wait)` returns a message, `Closed`, or `Nothing`. `Nothing` leaves the channel open.
- `recv` keeps its meaning: anything but a message within 30 seconds closes the channel and treats the broker as hung.
- The handshake still reads under the channel's timeout.

**`hyalos policy check|approve|show` manages the operator's policy.**
- `approve <file>` records the document through `Engine::approve_policy` as decided by a human, with `--reason` (default `policy.approved`). The human claim is the caller's until signed approvals (S5.4), as for holds.
- `check` validates without writing.
- `show` prints the policy in force.

**`hyalos-pr <owner/repo> <head> <base> <title> [<body-file>]` is in the image.**
- It posts the pull request to the broker through the forwarder.
- It is standard library only, like the other sandbox tools.
- It decides nothing: the broker checks the repository, head, base and content.

**The runtime directory** holds the broker's channel and the job sockets. It is `$HYALOS_RUNTIME_DIR`, else `$XDG_RUNTIME_DIR/hyalos`, else `run` in the data directory. It is made owner-only if missing.

## Consequences

The tests are `core/hyalos/tests/run_job.rs` and `run_image.rs`.

`run_job.rs` runs `run_job` with the real engine, policy, broker and App code. The GitHub is a stand-in on loopback: App endpoints, `git http-backend` and pull requests. The sandbox is a local process: the test executable, acting as the harness through the job's socket. It checks:
- the branch is at the stand-in;
- the pull request was opened with the token, for that branch into `main`;
- the token was revoked;
- `job.started` names `local-process` and `broker_held`;
- the egress refusal is in the log as `gate.answered`;
- neither credential is in the sandbox's environment or memory dump;
- the host refusals: not Linux, nothing ready, and a hanging runtime refused in well under the timeout, with nothing written;
- a revoke that fails makes the run fail.

`run_image.rs` runs `hyalos run` itself, with Codex in `hyalos-sandbox:test` under the container substrate and a scripted model.
- Codex runs one shell command: clone, commit, push, `hyalos-pr`, and curl to an unlisted host.
- While Codex waits for the model's next answer, the test dumps the sandbox from outside, as root with every capability, and scans the bytes on the host for the token and the provider key. The dump covers every process's environment, every readable mapping (about 500 MB) and `/tmp`. A control string, the job's branch name, is found in the environments, memory and files, which shows the scan reads the harness.
- It checks the branch, the pull request, the revoke, `job.started` with `container.docker` (or `.podman`), and curl's `egress.host_not_allowed`.
- The `sandbox image` workflow runs it, and now runs on changes to `core/hyalos` and `core/engine` too.

Not built, or accepted as is:
- **The issue comes from the command line, not from GitHub.** Reading issue `#N` through `read_issue` is the next step, and it makes the step's inputs forge-authored.
- **A refusal from the broker does not stop the sandbox.** The broker has already closed the job's socket, so the harness can reach nothing and ends on its own or at its deadline.
- **One job per command.** There is no daemon, no queue and no retry.
- **An interrupted run is not cleaned up.** Ctrl-C ends the command before it revokes the token or records `job.ended`; the token expires within the hour and the broker, its child, gets the same signal. Handling signals belongs to the long-running engine.
- **The Claude Code harness is wired, but only Codex is driven end to end** by the image test.
- **The engine's last events are written in order, not in one transaction.** A crash between them leaves a job that never ended in the log, which a later run does not repair.

The trusted core grows by about 900 lines (the `run` and `policy` commands, the channel's reader).
