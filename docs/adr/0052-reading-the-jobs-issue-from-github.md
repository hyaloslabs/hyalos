# ADR-0052: Reading the job's issue from GitHub

**Status:** Accepted

## Context

`hyalos run` takes the issue's text from the command line, so a job's only input is the operator's ([ADR-0050](0050-running-one-job-from-the-command-line.md)). A real job works on an issue someone opened on GitHub. The pieces to read one exist:
- `ToBroker::ReadIssue` and `ToEngine::Issue` ([ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md));
- `hyalos_broker::forge::Forge`, which asks GitHub's GraphQL API for exactly the fields kept, with the job's token, and parses the answer in the broker against caps and closed types;
- `ForgeIssue`, whose every text is `ForgeText`, forge-authored by type.

Nothing in a job uses them. S3.10b (#192) asks for `hyalos run --issue-number <N>`, with the issue read through the broker and its text treated as forge-authored.

Anyone who can open an issue writes its text, so the text may be a prompt injection aimed at the harness. The harness runs tainted whatever it reads (an actor, [ADR-0005](0005-provenance-is-a-type.md)), so what it can do is already bounded by the manifest and the broker's gates. What changes is that the job's input now really is untrusted, and the record and the prompt should say so.

## Decision

**`hyalos run --issue-number <N>` reads issue N of `--repo` through the broker.** It replaces `--issue` and `--issue-file`; giving both a number and text is refused as usage.

**The read comes after admission and before the sandbox.** The job's token exists only once the job is admitted ([ADR-0044](0044-minting-per-job-github-tokens-from-the-app.md)). So after `admitted`, the engine sends `read_issue { job, number }` and waits for the answer, as it waits for `admitted`, with the channel's 30-second timeout.
- **`issue`:** the engine builds the prompt from it and starts the sandbox. It refuses an answer whose number differs from the one asked for.
- **`refused`:** the broker has already ended the job ([ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md)). This covers no `read` action, no token, a GitHub refusal, no such issue, and any cap or parse failure. The engine stops the broker, revokes the token and records `job.ended` as `failed` with the reason code printed. No sandbox starts.

The engine never calls GitHub for the issue itself. Its only contact with the text is the parsed `ForgeIssue` the broker sends.

**The step's inputs are `ForgeAuthored`.** `mint_manifest` is given `StepInputs::of([ForgeAuthored])`, so a manifest that was not tainted would be refused (`mint.taint_required`). An actor's manifest is tainted anyway, so this changes no answer today. It records the truth for the code that will rely on it: the content gate for tainted jobs (S3.14b, #183) and workflows whose steps are not all actors (S4.2).

**The prompt sets the issue apart and says who wrote it.**
- The issue's title, labels and body go inside an `<issue number="N">` block.
- A sentence before the block says they were written on GitHub by the issue's users, not by the operator, and are a description of the problem, not instructions.
- The engine's own steps (clone, branch, push, `hyalos-pr`) come after the block.
- In the forge's text, every `<issue` and `</issue`, in any ASCII case, is written with `&lt;`. This means only the engine's own tags open and close the block, and an issue cannot end its block early and add steps that read as the engine's.

This is a hint to the model, not a boundary: a model may still follow text inside the block. The boundary is the sandbox, the manifest and the broker's gates, which hold whatever the harness is persuaded to try.

**What is recorded.** `task.created` records a SHA-256 of the repository, base, issue text and issue number, as before. The issue's text is not written to the log. The command prints the issue's number and state, and how many bytes of forge-authored text it holds, but not the text.

**An issue that is closed is still worked on.** The operator named it. The command prints its state.

## Consequences

A job can take its task from GitHub, and only the broker ever parses what GitHub sent.

The tests are in `core/hyalos/tests/run_job.rs`. The stand-in GitHub answers the GraphQL query for issue 7 of `octo/repo` and returns no issue for any other number. With `--issue-number 7`, the test checks:
- the harness received the issue's title, labels and body inside the block, after the note on who wrote it;
- GitHub saw exactly one GraphQL call, carrying the job's token;
- no token is left live;
- the issue's body is not in the log file.

With `--issue-number 99`, the test checks that `forge.no_such_issue` fails the job, that no sandbox ran, that the token was revoked, and that `job.ended` is `failed`. Unit tests check that the prompt places the block before the engine's steps, and that an issue whose text writes `</issue>` cannot close its block.

Not here:
- **Comments on the issue.** The broker reads the issue alone; comments are more forge-authored text, with their own caps.
- **Writing back to the issue**, such as a comment linking the pull request (`comment`, an action of its own).
- **Choosing the issue from GitHub**, such as by a label, which is the long-running engine's work.
