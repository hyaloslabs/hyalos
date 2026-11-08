# ADR-0078: Agent-started runs wait for the operator to release the issue

**Status:** Proposed

## Context

A run of the fix-issue workflow gives its actor only typed values and the operator's own words ([ADR-0062](0062-the-fix-issue-workflow-and-its-runner.md)). The issue's text reaches the actor only through a handle that a human released ([ADR-0063](0063-gated-handles-for-raw-text.md)). The only release is `hyalos run --release-issue`, which the operator gives in their own terminal, signed with the key unlocked before the run ([ADR-0070](0070-the-operators-signature-on-every-human-decision.md)).

`start_job` and `hyalos start` ([ADR-0069](0069-jobs-the-driving-agent-starts-as-mcp-tasks.md), [ADR-0075](0075-the-thin-cli-for-the-agents-tools.md)) start runs for the driving agent. The agent never holds the operator's key, so those runs have no operator text and no release. Their actor's prompt was "Resolve issue #N of owner/name" plus the triage and plan values.

On bot1 (#253, #257) the actor never learned what the issue asked. It searched the clone for the number, tried to fetch the issue through the broker (refused, correctly), then edited an unrelated file and spent its 200,000-token budget. The same issue with `--release-issue` opened a correct one-line pull request. So the first job the set-up skill starts, and every job an agent starts, could not succeed on an issue whose content matters.

## Decision

**A run the driving agent starts holds the issue, and its actor waits for the operator's release.** `start_job` and `hyalos start` set `Request.await_release` (`hyalos run --await-release` for a run in the operator's terminal). `--await-release` needs `--issue-number` and is not given with `--release-issue`. It is stored only when true, so earlier run documents keep their digest.

- **Triage holds it.** The step that reads the issue has the broker hold it, as `--release-issue` does (`ToBroker::HoldIssue`). The run then writes the digest, the repository and the issue's number to `runs/task-<id>.held` in the data directory (owner-only), and releases nothing. The runner keeps an exclusive lock on that file while it waits, and removes it however the wait ends.
- **The actor waits before anything is minted.** The actor's job starts and waits until the log shows a `handle.released` for that digest and the run's task. Its runner reads the log every half second (`Engine::catch_up`, now public), so it sees an event another process wrote. No manifest, token or sandbox exists while it waits.

  Three things end the wait the way they end any job: an interrupt, the agent's `tasks/cancel` (`hyalos stop`), and the run's deadline (`handle.unreleased`). The wait comes out of the run's deadline, so a late release leaves the actor less time. Once released, the actor runs as with `--release-issue`, its manifest naming the handle and its prompt saying how to read it.
- **`hyalos release <task> [--reason <code>]`** is the operator's release: a decision command, signed after the passphrase like `approve` and `cancel` (default reason `handle.operator_released`). It reads the digest from `task-<id>.held` and records it through `Engine::release_handle`. That refuses as ADR-0063 does: human only, task open, one release per handle and task. The command also refuses a task that already has a release. It is in `tools_cmd::DECISIONS`, and no MCP tool releases.

  It refuses before asking for the passphrase in two cases:
  - no runner holds the file's lock, because its runner stopped and nothing waits;
  - the issue no longer matches. The command reads the issue again with a read-only App token for that one repository (revoked after), and takes its digest as the broker does (`hyalos_broker::handles::hold`). It refuses unless that digest is the held one.

  The file is the runner's, but anything running as the operator's user could write it. The re-read makes that harmless: the operator releases only bytes equal to the issue they are told to read, as it is when they release it. An issue edited after triage, or a file naming another issue, is refused. The broker still serves bytes only for a digest it holds from this run's own read, and only to a manifest that names it (ADR-0063).
- **The agent sees the wait in the log alone.** `runs::awaiting_release(state, task, request)` is true when all four hold:
  - the request asked to wait;
  - every step before the actor returned its values;
  - the actor's job is running;
  - the log has no release for the task.

  `runs::Phase::Working` carries the issue's number then. `job_status` and `tasks/get` say the actor waits for the operator to release issue #N (`hyalos release <task>`, in their own terminal). `list_approvals` adds `releases`, in its output schema too, for runs whose runner is live: the task, the issue's number, the repository, and the commands `hyalos release <task>` and `hyalos stop <task>`. All of these are identifiers.
- **A held issue does not survive its runner.** The broker keeps the bytes in memory for its run. A run with `await_release` or `release_issue` whose runner stopped is ended, not resumed. `hyalos run --resume`, and the server's automatic resume, end the job the log shows running as `handle.lost`, then complete the task. They need no substrate and mint nothing, so the agent sees the run ended and can start the issue again.

**An actor given no issue text and no operator words says it is blocked.** If its prompt has no handle, the request has no text of the operator's, and the run works on an issue number, the prompt adds a line: the issue's text was not released; do not search for it or guess; if what it was given does not say what to change, change nothing and report that it is blocked. This covers `hyalos run --issue-number <n>` without `--release-issue` or `--await-release`, so it ends quickly rather than spending its budget.

## Consequences

An agent-started job now does the work the issue asks for, and the release stays the operator's decision. The operator reads the issue on the forge and runs one command in their own terminal, signed as every decision is. The agent can say what is waiting and which command decides it, but cannot release.

A run now waits on a person. While it waits, it holds a broker, a runner and its lock, but no token and no sandbox, until the run's deadline (`--minutes`, default 30). An operator who is away lets it time out, and the agent starts another.

A runner that stops while it waits ends its run (`handle.lost`). Keeping a held issue across runners would need the broker's bytes or a fresh hold, and a fresh hold could see an edited issue. That is left for later.

`hyalos release` makes a network call: one token and one read of the issue, from the operator's machine with the App's key. An issue edited between triage and the release cannot be released; the run is stopped and started again.

The set-up skill's first job now ends with the operator releasing the issue, so the timed run (#253) includes one more step.
