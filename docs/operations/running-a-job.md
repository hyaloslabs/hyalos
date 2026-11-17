# Running a job

`hyalos run` runs one job: a harness in a sandbox works on an issue in one repository, pushes the branch `hyalos/job-<id>` and opens a pull request ([ADR-0050](../adr/0050-running-one-job-from-the-command-line.md)). No credential enters the sandbox; the broker adds each one to the request it is for.

## 1. Before the first job

`hyalos setup` lists which of these are still missing and who takes each ([getting-started.md](getting-started.md)).

- **The engine's key.** Run `hyalos init`. See [keys-and-recovery.md](keys-and-recovery.md).
- **Your approval key.** Run `hyalos operator init`; approving the policy below and releasing an issue's text are your decisions, signed with it after your passphrase. See [approvals.md](approvals.md).
- **A provider key.** Write `provider.keys` and set a spend cap at the provider. See [provider-keys.md](provider-keys.md).
- **The GitHub App.** Write `github.app` and `github-app.pem`, and install the App on the repository. See [github-app.md](github-app.md). `hyalos github check <owner/repo>` shows the App can mint a token for it with each permission a job needs.
- **A substrate and the image.** With a release, pull the image it names by digest ([releases.md](releases.md)). From source, build the image (`docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .` from the repository root; without `--target sandbox` it builds the test image). `hyalos doctor` must say a job could run. A job runs under gVisor where it is set up, otherwise in a plain container ([substrates.md](substrates.md)).
- **A policy.** The shipped baseline allows nothing, so no job runs until you approve a policy that allows what a job asks for. A job asks for `read`, `push` and `open_pr`, and `request_host` for each `--allow-host`. It also asks for `data_flow` for each place its data may go ([ADR-0055](../adr/0055-data-flows-name-where-the-jobs-data-may-go.md)): `job->model` and `job->forge` always, and `job->web` when it may reach a host:

  ```
  permit (principal, action in [Action::"read", Action::"push", Action::"open_pr", Action::"data_flow"], resource);
  ```

  To keep a job that read forge content off the web whatever hosts it asks for, add:

  ```
  forbid (principal, action == Action::"data_flow", resource)
    when { principal.tainted && resource.name == "job->web" };
  ```

  Every actor is tainted, so with this rule a run with `--allow-host` is refused at minting, with a `no manifest` message that names `data_flow:job->web`.

  `hyalos policy starter` prints both, with `merge` added so that you can merge the pull request after you review it (section 3), and `hyalos setup policy` shows and approves it. `hyalos policy check policy.cedar` validates a policy of your own, `hyalos policy approve policy.cedar` puts it in force, and `hyalos policy show` prints what is in force. The baseline's forbids still apply under any policy: no write outside the job's repository, no push outside its branch, no irreversible action without an approval.

## 2. Run it

```
hyalos run --repo octo/repo --issue "The parser drops the last line of a file without a newline."
```

- `--issue-file <file>` reads the issue text from a file.
- `--issue-number <n>` runs the shipped fix-issue workflow on issue `n` of `--repo` ([ADR-0062](../adr/0062-the-fix-issue-workflow-and-its-runner.md)). It runs three jobs under one task, one after another:
  1. **triage**, a reader: the broker reads the issue from GitHub with a read-only token of its own and asks a model to classify it (`kind`, `size`, `clear`, `has_repro`).
  2. **plan**, a planner: the broker asks a model to choose an `approach` and whether to `add_test`, from those values only.
  3. **fix**, the actor: the harness in the sandbox. A pull request it opens closes issue `n`: the broker puts `Closes #n` first in its body, once the content gate has passed the harness's text ([ADR-0083](../adr/0083-a-fix-issue-pull-request-closes-its-issue.md)). When it is done it reports a result with `hyalos-submit` ([ADR-0064](../adr/0064-results-a-harness-submits-and-the-broker-reads.md)): `outcome` (`pr_opened`, `no_change` or `blocked`) and `tests_added`.

  The harness gets the issue's number, your text and the typed values. It never gets the issue's title or body, so describe the work with `--issue` if the classification is not enough. Without `--issue`, the task is "Resolve issue #n of owner/name". The reader and the planner call `--step-model`, which defaults to `gpt-5-mini` for Codex and `claude-haiku-5-5` for Claude Code, through the same `provider.keys`. Each spends at most 32,000 tokens. If a step cannot read its issue, or the model's answer is not exactly its fields, that job fails, no sandbox starts and every token is revoked. A step's job is recorded with the substrate `broker` and the attestation `none (attestation.no_sandbox)`.

  The run prints the actor's result as `result: outcome=..., tests_added=...`, and the job ends `step.submitted`. That result is the only thing taken from the harness. What it prints is not read, and is shown only if the job fails. A harness that exits 0 without submitting fails with `submit.missing`. A result that is not exactly those fields fails with the broker's `submit.*` code. Only the first submission counts.
- `--await-release` (with `--issue-number`) holds the issue as `--release-issue` does, then waits before the harness starts until you run `hyalos release <task>`, from another terminal, signed with your approval key ([ADR-0078](../adr/0078-agent-started-runs-wait-for-the-operator-to-release-the-issue.md)). Runs the driving agent starts always do this. If your policy permits `release_issue` and everyone who wrote the issue can write to the repository, the run releases it by your policy instead and does not wait ([ADR-0103](../adr/0103-policy-releases-an-issue-written-only-by-the-repository-s-writers.md), [approvals](approvals.md)). Without `--release-issue` or `--await-release` the harness gets only the typed values, and is told to report that it is blocked if they do not say what to change.
- `--release-issue` (with `--issue-number`) releases the issue's own text to the harness ([ADR-0063](../adr/0063-gated-handles-for-raw-text.md)). The reader's job has the broker hold the issue as it is then. The run prints the handle, the SHA-256 of those bytes, and records `handle.released` as your decision, with the digest and nothing of the text, signed with your approval key: the run asks for your passphrase before it writes anything (`--passphrase-stdin` reads it from stdin). The harness's prompt names the handle, and `hyalos-handle <digest>` in the sandbox prints the issue as JSON. The broker serves it only to a job whose signed manifest names that digest, so an edit to the issue after the hold is not served. Give it only for an issue whose text you are willing to put in front of the harness: a released issue can carry instructions an attacker wrote, and the harness reads them as text, under the same gates as any tainted job.
- `--base <branch>` is where the pull request goes (default `main`).
- `--harness claude-code` runs Claude Code instead of Codex.
- `--minutes` is the deadline (default 30); `--budget-tokens` the model budget (default 200,000).
- `--allow-host <host>` lets the job reach a web host through the egress gate, if policy allows it. Give the host name alone, in lower case (`docs.rs`): `hyalos run` (and `start_job`) refuses a port, a wildcard, a scheme or an address before the job starts, and the engine refuses to sign such a manifest (`mint.unknown_host`) ([ADR-0085](../adr/0085-the-capability-vocabulary.md)).

The command prints each step: the substrate and its attestation (none of today's substrates can show evidence beyond the engine's word, [ADR-0057](../adr/0057-recording-a-jobs-attestation.md)), the job's id, each gate answer the broker reports and the event it was recorded as, the harness's output (for a workflow's actor, only if its job failed), the revoke, and how the job ended, with the reason code `job.ended` records. It exits 0 only if the job succeeded and its token was revoked.

**Stopping a run.** Press Ctrl-C (or send the command `SIGTERM`) once. The run stops the sandbox, ends the job in the broker, revokes the token, records the job as `failed` with the reason `run.interrupted`, and exits 1 ([ADR-0053](../adr/0053-an-interrupted-run-ends-its-job.md)). That takes a moment; only if you cannot wait, press Ctrl-C again. The command then exits at once and says what it left undone: the token may stay valid until it expires, within an hour (revoke it as under `REVOKING THE FORGE TOKEN FAILED` below), the log does not say how the job ended, and the sandbox may run until its deadline.

**Resuming a run.** Each step's values are recorded with its job's ending, and the run writes `task.completed` when it is over (unless its last job is held). If the command stops without ending its job (killed, or the machine went down), `hyalos run --resume <task>` carries a fix-issue run on from the log ([ADR-0069](../adr/0069-jobs-the-driving-agent-starts-as-mcp-tasks.md)):
- It ends the job that was cut off as `failed` `engine.restarted`, or `deadline_exceeded` if the deadline has passed.
- It runs that step again in a new job, and the steps after it. It does not repeat finished steps.
- The deadline is still the one the task started with.

It needs the run's request in `runs/task-<id>.json` in the data directory, which `hyalos mcp` writes for the jobs an agent starts. A run started from the command line has none, so it cannot be resumed. The MCP server resumes its own runs by itself ([mcp.md](mcp.md)).

**Giving a job a skill.** An Agent Skill is a directory with a `SKILL.md` and its scripts, and a `capabilities.json` if it asks for any. Install it first, in your own terminal, since installing is your signed decision ([ADR-0089](../adr/0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md)):

```
hyalos plugin install ./pdf-tools
```

It prints what the skill asks for. If that adds a capability, read it and run the command again with `--grant`. A skill from a directory installs in local mode: it carries no signature or transparency-log entry, so it starts from no capability. That holds even when a published release of the same name already has some, and every update from the directory is diffed again ([ADR-0006](../adr/0006-plugins-are-skills-and-wasm-components.md)). A published skill installs by name from a plugin index, a release's assets or an offline bundle's directory: `hyalos plugin install --index <location> pdf-tools`. The install refuses an archive that is not exactly the one the index names ([ADR-0090](../adr/0090-a-plugin-index-served-as-release-assets.md)). Its Sigstore bundle is checked against Sigstore's trusted root as the marketplace pipeline last signed it. Each Hyalos release embeds one. A release directory carries a newer one, which the install imports first, but only if the one in force signed it. You can also import one with `hyalos plugin snapshot import <location>`, and `hyalos plugin snapshot show` prints the one in force. If an import says a new Hyalos release is needed, Sigstore has changed beyond what your snapshot can follow: update Hyalos ([ADR-0097](../adr/0097-the-trusted-root-snapshot-the-pipeline-signs-and-importing-a-newer-one.md)). A release directory also carries the marketplace's signed revocation list, which the same commands import. A release on it never installs, and an installed copy is disabled: `hyalos plugin list` marks it revoked, and `hyalos run --skill` refuses it until you install a release that is not revoked ([ADR-0098](../adr/0098-revoking-a-proven-malicious-release.md)). On a host with no network, download a release's files into a directory, carry it in, and install from it with `--index <dir>`: it is checked exactly as online. What a release is checked against has a maximum age: the snapshot 90 days and the revocation list 30, unless your install policy says otherwise (`--max-snapshot-age-days`, `--max-revocation-age-days`). Past it, nothing new installs or updates until you bring in a newer release, and what is installed keeps running ([ADR-0099](../adr/0099-installing-from-an-offline-bundle-within-a-maximum-age.md)). To let skills from an index you trust install without you, set an install policy: `hyalos plugin policy set --ceiling ceiling.json --index <location>`. A skill from that index asking for no more than `ceiling.json` then installs without your passphrase. Anything beyond the ceiling, from another index, or from a directory is still your decision ([ADR-0091](../adr/0091-the-install-policy-a-capability-ceiling-the-operator-signs.md)). `hyalos plugin update` checks each skill installed from an index for a newer version. Add `--every 24` to check daily, or run it from a timer. A newer version within your policy installs. One that asks for more waits for you in `hyalos plugin list`, and the installed version keeps running until you install the new one with `--grant` ([ADR-0092](../adr/0092-update-checks-that-keep-the-old-version-running.md)). Then `hyalos run ... --skill pdf-tools` puts the skill read-only in the job's sandbox, at `/opt/hyalos/skills`. The job may then do only what both it and the skill were granted, so a skill granted nothing leaves the job unable to push. `hyalos plugin list` shows what is installed. The microVM substrate cannot carry skills yet and refuses such a job. A skill can also be a third-party MCP server: an `mcp.json` beside its `SKILL.md` names the command, such as `{"command": "./server", "args": ["--stdio"]}`, and the install prints it. A job given the skill starts the server in its sandbox and registers it with Codex or Claude Code. Its network is the job's egress hosts, cut to what you granted the server ([ADR-0101](../adr/0101-a-third-party-mcp-server-runs-in-the-job-s-sandbox.md)). **Running your own workflow.** `hyalos run --repo <owner/name> --workflow <file> [--issue-number <n>] [--issue <text>]` runs a workflow document you wrote instead of the fix-issue workflow. Check it first with `hyalos workflow check <file>`, which prints the plugins whose grants bound each step. In a version 2 document, a `deterministic` step names an installed plugin whose `component.wasm` it runs in the broker, with no capability, on the values it reads, and returns typed fields. An actor may name an installed skill as its `plugin`. A step that reads a plugin's output, however indirectly, is bounded by that plugin's grant ([ADR-0102](../adr/0102-composing-skills-components-and-deterministic-steps.md)).

## 3. Merge the pull request

A job opens a pull request and merges nothing. Merging is yours ([ADR-0071](../adr/0071-merging-after-the-operators-approval.md)): review the pull request on GitHub, then in your own terminal:

```
hyalos merge owner/repo 42 --head <the full id of its head commit>
```

It asks for your passphrase and merges exactly that commit. A push after you reviewed makes GitHub refuse it, so you review again. Your policy must permit `merge`: the starter does from v0.1.2 ([ADR-0082](../adr/0082-the-starter-policy-permits-merge.md)). If yours was approved before, `hyalos setup policy` offers to replace it with the starter. [approvals.md](approvals.md#merging) has the options and what each refusal means.

## 4. If something goes wrong

- **Refused before the job started** (not Linux, no ready substrate, a container runtime that does not answer within 60 seconds, missing settings): nothing was written. The message says what to fix; `hyalos doctor` shows the substrates.
- **`no manifest: policy does not allow ...`**: the approved policy does not allow what the job asks for, such as `merge` under a policy approved before v0.1.2. The job is in the log as failed.
- **`REVOKING THE FORGE TOKEN FAILED`**: the token stays valid until it expires, within an hour. It is in no file and was never printed; only the broker, which has stopped, held it. To cut the repository off sooner, remove it from the App's installation ([github-app.md](github-app.md), "Rotating or revoking"). Then find out why the revoke failed: the network, or GitHub's status.
- **A gate refused something** (`gate Egress answered Deny`): the job asked for something its manifest does not allow. `flow.not_declared` means it tried to send data somewhere it has no flow to, such as a web host without `--allow-host`. It is recorded as `gate.answered`; the job carries on without it. A push or pull request holding a credential or hidden text is the content gate's refusal (`content.credential`, `content.hidden_text`, [ADR-0054](../adr/0054-the-content-gate-on-what-a-push-carries.md)). A pull request from the actor, which is tainted, is also refused for a key of a known shape (`content.key_shape`), a credential encoded or split (`content.encoded_credential`), a link off the forge or one with a query (`content.link`), or a title or body over 20,000 characters (`content.too_long`, [ADR-0065](../adr/0065-the-content-gate-on-what-a-tainted-job-publishes.md)); link to the repository's own issues and files, and name anything else in words.
- **`held; stopping the sandbox`**: a gate could not answer, such as the content gate on a push it could not read (`content.pack_unreadable`, `content.pack_too_large`, `content.ref_delta`). The job ends `held`, the sandbox is stopped and the token is revoked. The hold is in the log, and only a person can release it.

`hyalos log verify` checks the log afterwards.
