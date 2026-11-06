# Running a job

`hyalos run` runs one job: a harness in a sandbox works on an issue in one repository, pushes the branch `hyalos/job-<id>` and opens a pull request ([ADR-0050](../adr/0050-running-one-job-from-the-command-line.md)). No credential enters the sandbox; the broker adds each one to the request it is for.

## 1. Before the first job

- **The engine's key.** Run `hyalos init`. See [keys-and-recovery.md](keys-and-recovery.md).
- **A provider key.** Write `provider.keys` and set a spend cap at the provider. See [provider-keys.md](provider-keys.md).
- **The GitHub App.** Write `github.app` and `github-app.pem`, and install the App on the repository. See [github-app.md](github-app.md). `hyalos github check <owner/repo>` shows the App can mint a token for it.
- **A substrate and the image.** Build the image (`docker build -f images/sandbox/Containerfile -t hyalos-sandbox .` from the repository root). `hyalos doctor` must say a job could run. A job runs under gVisor where it is set up, otherwise in a plain container ([substrates.md](substrates.md)).
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

  `hyalos policy check policy.cedar` validates it, `hyalos policy approve policy.cedar` puts it in force, and `hyalos policy show` prints what is in force. The baseline's forbids still apply under any policy: no write outside the job's repository, no push outside its branch, no irreversible action without an approval.

## 2. Run it

```
hyalos run --repo octo/repo --issue "The parser drops the last line of a file without a newline."
```

- `--issue-file <file>` reads the issue text from a file.
- `--issue-number <n>` works on issue `n` of `--repo` instead. The broker reads it from GitHub with the job's token, and the harness is told that its text was written by the issue's users, not by you ([ADR-0052](../adr/0052-reading-the-jobs-issue-from-github.md)). If it cannot be read (no such issue, or GitHub refuses), the job fails before a sandbox starts and the token is revoked.
- `--base <branch>` is where the pull request goes (default `main`).
- `--harness claude-code` runs Claude Code instead of Codex.
- `--minutes` is the deadline (default 30); `--budget-tokens` the model budget (default 200,000).
- `--allow-host <host>` lets the job reach a web host through the egress gate, if policy allows it.

The command prints each step: the substrate, the job's id, each gate answer the broker reports and the event it was recorded as, the harness's output, the revoke, and how the job ended, with the reason code `job.ended` records. It exits 0 only if the job succeeded and its token was revoked.

**Stopping a run.** Press Ctrl-C (or send the command `SIGTERM`) once. The run stops the sandbox, ends the job in the broker, revokes the token, records the job as `failed` with the reason `run.interrupted`, and exits 1 ([ADR-0053](../adr/0053-an-interrupted-run-ends-its-job.md)). That takes a moment; only if you cannot wait, press Ctrl-C again. The command then exits at once and says what it left undone: the token may stay valid until it expires, within an hour (revoke it as under `REVOKING THE FORGE TOKEN FAILED` below), the log does not say how the job ended, and the sandbox may run until its deadline.

## 3. If something goes wrong

- **Refused before the job started** (not Linux, no ready substrate, a container runtime that does not answer within 60 seconds, missing settings): nothing was written. The message says what to fix; `hyalos doctor` shows the substrates.
- **`no manifest: policy does not allow ...`**: the approved policy does not allow what the job asks for. The job is in the log as failed.
- **`REVOKING THE FORGE TOKEN FAILED`**: the token stays valid until it expires, within an hour. It is in no file and was never printed; only the broker, which has stopped, held it. To cut the repository off sooner, remove it from the App's installation ([github-app.md](github-app.md), "Rotating or revoking"). Then find out why the revoke failed: the network, or GitHub's status.
- **A gate refused something** (`gate Egress answered Deny`): the job asked for something its manifest does not allow. `flow.not_declared` means it tried to send data somewhere it has no flow to, such as a web host without `--allow-host`. It is recorded as `gate.answered`; the job carries on without it. A push or pull request holding a credential or hidden text is the content gate's refusal (`content.credential`, `content.hidden_text`, [ADR-0054](../adr/0054-the-content-gate-on-what-a-push-carries.md)).
- **`held; stopping the sandbox`**: a gate could not answer, such as the content gate on a push it could not read (`content.pack_unreadable`, `content.pack_too_large`, `content.ref_delta`). The job ends `held`, the sandbox is stopped and the token is revoked. The hold is in the log, and only a person can release it.

`hyalos log verify` checks the log afterwards.
