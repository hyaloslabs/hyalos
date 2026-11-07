# ADR-0069: Jobs the driving agent starts, as MCP tasks that outlive the server

**Status:** Proposed

## Context

S5.2 (#30): "As a driving agent, I start a job as an MCP Task and poll its progress." Its acceptance is:
1. A Task survives an engine restart and resumes from the log.
2. For a client that does not advertise the Tasks extension, the same start, status and result operations work as plain tools with the same semantics ([ADR-0010](0010-mcp-primary-interface-two-principals.md)).

What shapes the decision:
- **The server holds no key.** [ADR-0067](0067-the-mcp-server-and-the-conformance-suite.md) has `hyalos mcp` read the log with `engine.pub` alone, and [ADR-0068](0068-the-driving-agents-credential.md) relies on that: the process the driving agent talks to should hold nothing that writes or signs. A run needs `engine.key`, the GitHub App's key and `provider.keys`.
- **A run takes minutes**, and a stdio server lives only as long as its client. The run must not end because the client closed.
- **The log had one writer.** Each `Engine` keeps the state in memory. Two processes appending to one log could each check an event against a stale state, or stamp one earlier than an event the other just wrote, which replay refuses (`TimeWentBackwards`, [ADR-0024](0024-event-origins-and-time-order.md)).
- **A run's progress was not all in the log.** The values a reader or planner returned were held in memory, and only the job's outcome was recorded. A run cut off after the readers could not carry on without asking the model again.
- **The Tasks extension** (SEP-2663, in the 2026-07-28 revision as `io.modelcontextprotocol/tasks`) is negotiated per request, from the client capabilities in `_meta`. `tools/call` answers a declaring request with a `CreateTaskResult` (`resultType: "task"`), and the client polls with `tasks/get`, which returns the result inline once the task is terminal. `tasks/cancel` and `tasks/update` are acknowledged with `resultType: "complete"`. `tasks/result` and `tasks/list` were removed. A tool that runs only as a task refuses a request that does not declare the extension with `-32021`, naming it in `data.requiredCapabilities`. A tool error is `completed` with `result.isError`, and `failed` is kept for protocol errors.
- **The driving agent's words must not steer a job** (ADR-0059). What it passes must be identifiers that the engine checks, never free text that reaches a prompt.

## Decision

**The server starts a runner, a process of its own that holds the keys.**
- `start_job` writes the run's request to `runs/start-<nonce>.json` in the data directory (0700; nonce 16 random bytes). It then starts this executable as `hyalos run --mcp-start <nonce>`:
  - in its own process group, with stdin `/dev/null` and stderr to `start-<nonce>.log`;
  - with stdout a pipe, and without `HYALOS_AGENT_CREDENTIAL` in its environment.
- The runner locks `start-<nonce>.lock` (`flock`, held for the whole run) and reads the request. It then runs as `hyalos run` does.
- Once `task.created` is in the log, `Setup::on_task` renames the lock, log and request to `task-<id>.*` (lock first, request last). Only then does it print `task <id>` on stdout. So the task the server answers with is always in the log, and `tasks/get` resolves it at once.
- The server waits at most 90 s for that line, which covers substrate detection's 60 s. Without it, the call is a tool error: `the job did not start:` followed by the runner's last `hyalos:` line. The server kills the runner and removes its files, and the log is unchanged. A thread reaps the runner, which outlives the server.
- `hyalos run --mcp-start` is for the server; operators do not use it.

**The agent gives identifiers only.**
- `start_job` takes `repo` (`owner/name`), `issue_number` (positive) and an optional `base`. Any other field is refused.
- Everything else comes from the template the operator gives `hyalos mcp`: `--base`, `--harness`, `--image`, `--budget-tokens`, `--minutes` and `--step-model`, parsed as `hyalos run` parses them.
- `run_cmd::check` checks the request; `is_branch` allows at most 100 bytes of `[A-Za-z0-9._/-]`, with no `..`, `//`, `.lock`, leading `-`, `/` or `.`, and no trailing `/` or `.`.
- The run is the fix-issue workflow with no text of the operator's, so its task line stays "Resolve issue #n of owner/name". No text the agent chose reaches a prompt, a manifest or the log.
- The request document is `run_cmd::Stored` (version 1; it never holds the `command` test harness). `task.created.request` is the SHA-256 of its JSON (`request_digest`), so a document that does not match its task is never used, to resume it or to show it.

**Several processes may write one log.**
- An engine now remembers the head it has seen. Before it checks an event of its own, `Engine::catch_up` reads what was appended after that head (`Log::events_after`, which verifies sequence, chain, hash and signature) and replays it into its state.
- It then appends only onto that head (`Log::append_all_after`). If another process got there first, the append fails with `LogError::Moved`, and the engine catches up and tries again, at most 8 times.
- So an event is always checked against the latest state, and against the latest time. Stamping an event earlier than another process's last one is `TimeWentBackwards`, refused before anything is written. `compact` catches up first too.
- `hyalos run` stamps its first event at the later of the system time and the state's last time.

**Each step's values are in the log, and every run is recorded over.**
- `job.values { job, values }` (engine-only, at most 16 `TypedValue`s, once, while the job runs) is written in one transaction with the `job.ended` of a reader or planner that answered, and of an actor whose submission was taken (`step.submitted`).
- This is schema version 12. It adds the kind and changes no payload.
- `StateImage` version 7 keeps each job's values and its ending's reason. An older image reads as no values and reason `legacy.unrecorded`.
- `hyalos run` now writes `task.completed` at the end of every run, from the CLI too. The one exception is a run whose last job ended held, which an operator still decides.

**A run whose runner stopped resumes from the log.**
- `hyalos run --resume <task>` locks `task-<id>.lock` and exits 0 at once if another runner holds it. It reads the request and checks its digest against the task. It does nothing for a task that is no longer open.
- It ends the job that was running when the runner stopped: `failed` `engine.restarted`, or `deadline_exceeded` `job.deadline` if the run's deadline (task creation plus `--minutes`) has passed.
- It rebuilds the values of the steps that succeeded with values, in order, through the same `Runnable::accept` as a live run. Those steps are not run again.
- It runs the step that was cut off in a new job, then the rest, with the same deadline.
- A run with `--release-issue` is not resumed: its handle lived in the broker that stopped. Neither is one with no issue number.
- The server starts `--resume` for every unfinished task of its runs whose lock no one holds, when it starts and whenever `tasks/get`, `job_status` or `tasks/cancel` asks about one. The output goes to the end of `task-<id>.log`.
- The operator may run `--resume` by hand.

**Cancelling is a file the runner watches.**
- `tasks/cancel` creates `task-<id>.cancel` for an unfinished task. `Interrupt::watch` makes its appearance count as the first signal does (ADR-0053), named `tasks/cancel`.
- The job the run is in ends `failed` `run.cancelled`, with the sandbox stopped, `end_job` sent and the token revoked, and the run then writes `task.completed`.
- Cancelling an ended task, or cancelling twice, changes nothing. A cancel for a task with no runner starts `--resume`, which sees the file and ends.

**The MCP server offers the Tasks extension.**
- `hyalos::mcp::Tool` has `task: TaskSupport` (`Never`, `Optional`, `Required`).
- `Tools` gains `start`, `task` and `cancel`, which default to having no tasks. `server/discover` advertises `capabilities.extensions["io.modelcontextprotocol/tasks"]` when any tool supports tasks.
- A request that declares the extension and calls an `Optional` or `Required` tool gets a task:
  - `resultType: "task"`, `taskId`, `status`, `createdAt` and `lastUpdatedAt` (RFC 3339 UTC), `ttlMs`, `pollIntervalMs` and `statusMessage`.
  - A start refused at once is the call's own result.
- A request that does not declare it never gets a task. An `Optional` tool runs as a plain call; a `Required` one is `-32021` with `data.requiredCapabilities`. A legacy `task` parameter asks for nothing.
- `tasks/get`, `tasks/update` and `tasks/cancel` check the extension first, then the `taskId` (`-32602` for an unknown one).
  - `tasks/get` answers `resultType: "complete"` with the task, and its `result` or `error` once it has one.
  - `tasks/update` and `tasks/cancel` answer the bare acknowledgement.
  - On HTTP, `Mcp-Name` must equal the `taskId`.
  - `tasks/result` and `tasks/list` are `-32601`.

**The engine's tools are `start_job`, `job_status` and `job_result`.**
- `start_job` is `Optional`. For a client without the extension, it answers the same task, as `tasks/get` shows it, as structured content.
- `job_status { task }` is `tasks/get` as a plain tool. `job_result { task }` is the terminal result, or a tool error while the task is working or after it was cancelled.
- A task's id is its sequence number in decimal. Only the server's own runs (a request document that matches) are tasks; any other id is "no such task".
- The status is read from the log and the request document alone, with the public key:
  - `working`, with "n of 3 steps done", and a note when its runner stopped, until `task.completed` or a held last job.
  - `cancelled`, if `task.cancelled` was recorded or the last job ended `run.cancelled`.
  - `completed`, with `{task, outcome: "succeeded", result}` when all three steps returned values. The result is the actor's values by field, a choice shown as the workflow's own option text.
  - Otherwise `completed`, with `isError` and `{task, outcome, reason}`, the last job's outcome and reason code.
- `ttlMs` is null, since tasks are the log's and kept as long as it is, and `pollIntervalMs` is 5,000.
- `list_tasks` is unchanged.

**The conformance suite runs the Tasks scenarios.**
- `crates/mcp-conformance` serves the suite's task fixtures through the same layer: `greet`, `slow_compute`, `failing_job`, `protocol_error_job`, and `confirm_delete` as a plain call. Their tasks live in memory, with each one's state following from the time since it started.
- `tests/conformance.rs` runs each of the 10 `tasks-*` scenarios with `--scenario` and `expected-failures-tasks.yml`. That file lists the four checks that need a task to ask the client for input (`tasks-mrtr-input` three, `tasks-mrtr-composition` one), which the server never does. `tasks-status-notifications` is skipped by the suite itself.

## Consequences

- The driving agent can start work that spends the provider budget and opens pull requests. It is bounded by the operator's template, the policy that mints each manifest, and the repositories the App is installed on. Like `list_tasks`, any admitted agent can read and cancel any of the server's tasks; credentials are not yet scoped to tasks.
- The server still never opens `engine.key`, but it can start a process that does. The runner gets the server's environment, minus the agent's credential.
- A runner outlives the server and the client. A crash releases its lock, since `flock` locks die with their process, so the next `tasks/get` resumes the run. The runs directory must be on a local file system where `flock` works.
- A runner stopped mid-job leaves that job's GitHub token live until GitHub expires it (one hour), since nothing revokes it. Its broker ends when its channel closes, and a container sandbox may run to its deadline. The resumed actor works on a new branch (`hyalos/job-<new id>`), so a cut actor may leave a branch, or a pull request, behind.
- Two engines can now share one log safely. Each write costs a read of the events after the head the engine last saw: none, when it is the only writer.
- Runs started by the CLI also end with `task.completed`, so `list_tasks` and the state show them finished. Old logs keep their open tasks.
- Every resume reads the same request document. Changing the template does not change a run already started, and a document edited by hand is refused by its digest.
- `tasks/update` does nothing, since no task asks for input. When one does, approvals still go through the operator's channel, never the client ([ADR-0012](0012-humans-approve-only-irreversible-actions.md)).
