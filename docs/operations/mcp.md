# The MCP server

`hyalos mcp` is the engine's MCP server, on revision 2026-07-28 ([ADR-0010](../adr/0010-mcp-primary-interface-two-principals.md), [ADR-0067](../adr/0067-the-mcp-server-and-the-conformance-suite.md)). Its tools:
- `start_job` runs the fix-issue workflow on a GitHub issue, as `hyalos run --issue-number` does ([ADR-0069](../adr/0069-jobs-the-driving-agent-starts-as-mcp-tasks.md)).
- `job_status` and `job_result` read that job, for clients without the Tasks extension.
- `list_tasks` returns the newest tasks in the event log, with each task's status and the outcome of each of its jobs.
- `list_approvals` returns the holds waiting for your decision, with the command you run for each ([ADR-0074](../adr/0074-approvals-shown-to-the-driving-agent.md)).

## What waits for you

When a gate cannot decide, the job stops and a hold waits for a decision. `list_approvals` shows the driving agent each open hold:
- which gate held and why;
- how much the held action matters (its tier, and whether it touches credentials, policy, publishing or a capability grant);
- who may release it;
- the two commands you can run: `hyalos approve <hold>` to release it, or `hyalos cancel <task>` to end the task instead.

A held run's `job_status` shows the same for its own holds. The agent can show you this and ask you to run the command, but it cannot decide anything: no tool releases, cancels or merges, and every decision is signed with your key at your own terminal ([Approvals](approvals.md)). The details are ids, codes and a digest, never text from the issue or a model. A client that reads only text gets them as sentences.

No client renders MCP Apps yet (see below), so there is no graphical approval view.

## The same tools on the command line

Each tool has one command that makes the same call and prints the same answer ([ADR-0075](../adr/0075-the-thin-cli-for-the-agents-tools.md)):

| Command | Tool |
| --- | --- |
| `hyalos tasks [--limit <n>]` | `list_tasks` |
| `hyalos start <owner/repo> <issue-number> [--base <branch>] ...` | `start_job` |
| `hyalos status <task>` | `job_status` |
| `hyalos result <task>` | `job_result` |
| `hyalos approvals [--limit <n>]` | `list_approvals` |
| `hyalos stop <task>` | `tasks/cancel` |

- Each prints the tool's text; `--json` prints its structured content instead, and is always JSON.
- They exit 1 when the tool answers with an error, and 2 for bad usage.
- `start` takes the same options as `hyalos mcp` and starts the runner in the background, as `start_job` does; `hyalos run` is still the runner in your terminal.
- `stop` asks a run started by `start` or `start_job` to stop, as `tasks/cancel` does. It is not `hyalos cancel`, which is your signed decision on a held task.
- They need no agent credential: they run as you and decide nothing. Like the server, `start` starts a runner, and `status` or `result` on a run whose runner stopped resumes it (`hyalos run --resume <task> --auto`), which writes to the log with the engine's key.

Your decisions are the other way round: they are commands only, and no MCP tool makes one. These are `approve`, `cancel`, `merge`, `policy approve`, `agent issue`, `agent revoke` and `run --release-issue`. A test fails if the server gains a tool without a command here, or a tool that decides. Setup and maintenance (`init`, `doctor`, `operator`, `github check`, `log ...`, `workflow check`) have no MCP tool either.

## Which clients can connect

A client must speak MCP revision 2026-07-28: it sends `_meta` with the protocol version and its capabilities on every request (usually starting with `server/discover`), and never `initialize`. A client that opens with `initialize`, the 2025-11-25 handshake, is refused with `-32601` and told which revision this is. Hyalos does not serve older revisions ([ADR-0073](../adr/0073-clients-on-older-mcp-revisions-are-unsupported.md)).

Checked November 7, 2026, in spike #92 ([findings](../planning/spike-92-findings.md)); re-check when a client changes version.

| Client | Version checked | Connects |
| --- | --- | --- |
| Claude Code | 2.1.318 | Yes. It calls every tool plainly, so it uses `job_status` and `job_result`, not `tasks/get`. |
| Hermes Agent | 0.19.0 | **No.** It uses the Python MCP SDK 1.26.0 and `initialize`. It will need Python MCP SDK 2.3.0 or later and a change to call `discover()`. |

Neither client renders MCP Apps, and the server offers none: everything it returns is plain text and structured content, approvals included.

The server reads the log with the engine's public key only. It never opens `engine.key`, so it runs while `hyalos run` writes. Run `hyalos init` first, and set up everything `hyalos run` needs (see [Running a job](running-a-job.md)) before the agent starts a job.

## Issuing the agent a credential

Every call needs a credential you issued ([ADR-0068](../adr/0068-the-driving-agents-credential.md)). Issue one per agent:

```
hyalos agent issue claude-code
```

This prints the credential on stdout, once. The log keeps only its SHA-256, so it cannot be shown again: if you lose it, revoke it and issue another. Treat it like a password and give it only to the agent.

```
hyalos agent list            # every credential issued, by id and name, and whether it is revoked
hyalos agent revoke <id>     # the server refuses it from its next call; no restart needed
```

A name belongs to one credential in force at a time. To replace a credential, revoke the old one, then issue the new one under the same name. A revoked credential never works again.

Without a credential in force, the server answers every call 401 (`-32030`). If it cannot read the log to check, it answers 503 and says why on its stderr.

The credential lets the agent call the server. It cannot approve anything: a decision of yours is written only by a command you run, signed with your approval key after your passphrase ([approvals.md](approvals.md)), never through the server. Each `hyalos agent issue` and `revoke` asks for that passphrase.

## From a client that starts it (stdio)

This is the default, and there is no listener at all. The server takes the credential from `HYALOS_AGENT_CREDENTIAL`. In Claude Code:

```
claude mcp add hyalos -e HYALOS_AGENT_CREDENTIAL=hyalos-agent-… -- hyalos mcp
```

Set `HYALOS_DATA_DIR` and `HYALOS_CONFIG_DIR` in the client's server entry too if your log and key are not in the default places. The server checks the credential when it starts and exits with the reason if it is missing, unknown or revoked, so a wrong setup shows at once. Run `hyalos mcp` by hand to check it: it reads one JSON-RPC message per line on stdin and answers on stdout until stdin ends.

## Over HTTP on loopback

```
hyalos mcp --listen 127.0.0.1:8765
```

This serves Streamable HTTP at `http://127.0.0.1:8765/mcp`. Only a loopback address is accepted. A request whose Host or Origin is not that address and port is refused with 403, so a web page cannot reach the server by rebinding a name to 127.0.0.1.

Each request needs `Authorization: Bearer <credential>`. Without a credential in force it is 401 with `WWW-Authenticate: Bearer`. Any process on the machine can reach the port, so the credential is what keeps it out. Turn HTTP on only while you need it.

## Jobs the agent starts

The agent gives `start_job` a repository (`owner/name`) and an issue number. Nothing else: no text of its own reaches the job. The pull request's base branch is yours, set with `--base` (default `main`). Everything else comes from options you give `hyalos mcp`, as for `hyalos run`:

```
hyalos mcp --harness claude-code --minutes 30 --budget-tokens 200000 --base main
```

The other options are `--image` and `--step-model`.

The server does not run the job itself. It starts `hyalos run` as a process of its own, which holds the keys and writes the log, and answers once the task is in the log. That runner outlives the server: closing the client does not stop the job. Its files are in `runs/` in the data directory:
- `task-<id>.json`: the request, which the log records the digest of.
- `task-<id>.log`: what the runner printed. Read it when a job fails.
- `task-<id>.lock`: held while a runner has it.
- `task-<id>.failed`: the error its last runner stopped with, if it did.

`hyalos log compact` and `hyalos log prune` refuse while a runner holds its lock. Wait for the run to end, or stop its runner, first.

The answer is a task:
- **A client with the Tasks extension** polls it with `tasks/get` and stops it with `tasks/cancel`.
- **A client without it** gets the task's id and status from `start_job` and polls with `job_status` and `job_result`.

The task is `working` while the steps run, then:
- `completed` with the result the harness submitted (`outcome`, `tests_added`), or
- `completed` as a tool error with the last job's outcome and reason code, or
- `cancelled`.

A job that ends held stays `completed` with its reason until you decide the hold.

The result is marked as untrusted data ([ADR-0072](../adr/0072-marking-untrusted-data-in-results.md)). Its values are typed: a truth value, a number, or one of the options the workflow names. So no text from the issue or a model is in it. But the harness that chose them read the issue, so they could be steered by whoever wrote it:
- The structured result says `"provenance": "model_from_tainted"` and `"untrusted": true`.
- A client that reads only text gets a notice that the data is not instructions, then the values inside an `<untrusted-data provenance="model_from_tainted">` element that they cannot close.

No answer holds a credential, a key or a token. Answers are built from the log's typed state and Hyalos's own codes.

If a runner stops (a crash, a reboot), the task resumes from the log:
- Finished steps are not run again; the step that was cut off runs in a new job.
- The server starts `hyalos run --resume <task>` when it starts, and whenever the agent asks about such a task. You can run it yourself too.
- The cut job's GitHub token was not revoked and stays live until GitHub expires it (one hour). Check the repository for a branch or pull request it left behind.
- A runner that stopped with an error (not a crash) is not resumed by itself: the task says it waits for you, and `runs/task-<id>.failed` and `task-<id>.log` say why. Fix that, then run `hyalos run --resume <task>`.

If `start_job` fails at once, its message is the runner's reason, as `hyalos run` would print it. Nothing is written to the log in that case.

## Conformance

CI runs the official MCP conformance suite against the protocol layer on every pull request (job `mcp conformance`). To run it locally:

```
npm ci --ignore-scripts --prefix crates/mcp-conformance
HYALOS_REQUIRE_MCP_CONFORMANCE=1 cargo test -p hyalos-mcp-conformance
```

`crates/mcp-conformance/expected-failures.yml` lists the features the server does not offer: completions, resources, prompts and input-required results. Every other scenario the revision requires must pass. The Tasks extension's scenarios run one at a time, with `expected-failures-tasks.yml`, which lists the four checks of a task asking the client for input.
