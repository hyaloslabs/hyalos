# ADR-0113: Skills drafted from successful runs are tainted and read-only

**Status:** Accepted

## Context

S9.2 (#58), the second story of E9 (#56): the engine drafts a skill from a successful run. Its acceptance: the draft is tainted, and it cannot run with more than read capabilities. [ADR-0005](0005-provenance-is-a-type.md) says what a model chose after reading the forge is tainted, and [ADR-0112](0112-learnings-queried-over-the-event-log.md) already treats a job's values that way. A skill is files a job is given ([ADR-0089](0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md)), and until now only a human or the install policy put one in front of a job, each with a grant.

A draft must not open a new way in. If it held text a model or the forge wrote, a later job would read that text as instructions from a trusted skill. If it carried a grant of its own, a run could widen what later runs may do without the operator.

## Decision

**When a workflow run succeeds, Hyalos drafts a skill from it.** After `task.completed`, when the last job ended `succeeded`, `hyalos run` writes `draft-<task>`. A draft has three files:
- `SKILL.md` lists each step with an output: its id, role, and the value of each field. A choice is given as the option the workflow's author wrote, never free text. A draft is a pure function of the workflow, the typed values in the log, and the run's request, so the same log always drafts the same bytes.
- `capabilities.json` asks for `read` and `job->model` only.
- `run.json` holds the run's stored request, whose SHA-256 is the digest `task.created` records, so the draft can be checked against the log alone.

The files are read back as any skill is and kept in the plugin store by digest. A draft that cannot be written is printed and leaves the run as it ended.

**The log records it as `skill.drafted { name, digest, task, grant }`** (schema version 23), written by the engine only. It is not a decision and grants nothing a job did not already have. The state refuses a draft with `DraftRefused` in each of these cases:
- its task is not completed;
- its task's last job did not succeed;
- its name is already drafted or installed;
- its grant is not within `Grant::read_only()`: actions `{read}` and flows `{job->model}`, with no hosts or tools.

The state keeps the drafts by name (`StateImage` version 16), so they survive compaction.

**A draft is tainted, and a job given it is held to read.** `hyalos run --skill draft-<task>` loads a draft when no plugin of that name is installed. It is checked against its recorded digest and bounded by the read-only grant it was drafted with, whatever its files ask. A job may do only what both it and its skills were granted ([ADR-0089](0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md)), so a job given a draft cannot push, open a pull request or comment. Its words came from the workflow's author and its choices from a model that read the forge, so it stays `model_from_tainted` until a later step proves otherwise. S9.3 makes it trusted only after it is replayed in simulation. Widening it is still a human's `hyalos plugin install` with `--grant`.

## Consequences

Every successful workflow run leaves a skill a later job can be pointed at, and its source is checkable: `task` names the run, and `run.json` hashes to that task's request. No new decision, wire message or manifest field is added. A draft's text is only as useful as the workflow's field names and options: it records what worked, not why. The plugin store gains one small directory per successful run. `hyalos plugin list` does not show drafts yet.
