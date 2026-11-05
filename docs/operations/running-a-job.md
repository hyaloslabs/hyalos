# Running a job

`hyalos run` runs one job: a harness in a sandbox works on an issue in one repository, pushes the branch `hyalos/job-<id>` and opens a pull request ([ADR-0050](../adr/0050-running-one-job-from-the-command-line.md)). No credential enters the sandbox; the broker adds each one to the request it is for.

## 1. Before the first job

- **The engine's key.** Run `hyalos init`. See [keys-and-recovery.md](keys-and-recovery.md).
- **A provider key.** Write `provider.keys` and set a spend cap at the provider. See [provider-keys.md](provider-keys.md).
- **The GitHub App.** Write `github.app` and `github-app.pem`, and install the App on the repository. See [github-app.md](github-app.md). `hyalos github check <owner/repo>` shows the App can mint a token for it.
- **A substrate and the image.** Build the image (`docker build -f images/sandbox/Containerfile -t hyalos-sandbox .` from the repository root). `hyalos doctor` must say a job could run.
- **A policy.** The shipped baseline allows nothing, so no job runs until you approve a policy that allows what a job asks for. A job asks for `read`, `push` and `open_pr`, and `request_host` for each `--allow-host`:

  ```
  permit (principal, action in [Action::"read", Action::"push", Action::"open_pr"], resource);
  ```

  `hyalos policy check policy.cedar` validates it, `hyalos policy approve policy.cedar` puts it in force, and `hyalos policy show` prints what is in force. The baseline's forbids still apply under any policy: no write outside the job's repository, no push outside its branch, no irreversible action without an approval.

## 2. Run it

```
hyalos run --repo octo/repo --issue "The parser drops the last line of a file without a newline."
```

- `--issue-file <file>` reads the issue text from a file.
- `--base <branch>` is where the pull request goes (default `main`).
- `--harness claude-code` runs Claude Code instead of Codex.
- `--minutes` is the deadline (default 30); `--budget-tokens` the model budget (default 200,000).
- `--allow-host <host>` lets the job reach a web host through the egress gate, if policy allows it.

The command prints each step: the substrate, the job's id, each gate answer the broker reports and the event it was recorded as, the harness's output, the revoke, and how the job ended. It exits 0 only if the job succeeded and its token was revoked.

## 3. If something goes wrong

- **Refused before the job started** (not Linux, no ready substrate, a container runtime that does not answer within 60 seconds, missing settings): nothing was written. The message says what to fix; `hyalos doctor` shows the substrates.
- **`no manifest: policy does not allow ...`**: the approved policy does not allow what the job asks for. The job is in the log as failed.
- **`REVOKING THE FORGE TOKEN FAILED`**: the token stays valid until it expires, within an hour. It is in no file and was never printed; only the broker, which has stopped, held it. To cut the repository off sooner, remove it from the App's installation ([github-app.md](github-app.md), "Rotating or revoking"). Then find out why the revoke failed: the network, or GitHub's status.
- **A gate refused something** (`gate Egress answered Deny`): the job asked for something its manifest does not allow. It is recorded as `gate.answered`; the job carries on without it.

`hyalos log verify` checks the log afterwards.
