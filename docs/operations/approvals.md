# Your approval key and your decisions

Every decision recorded as yours is signed with a key only you can use, as well as the engine's key: releasing a hold, cancelling a held task, approving a policy, issuing or revoking an agent's credential, and releasing an issue's text to a run. `hyalos log verify` checks each one. A decision the engine's key alone wrote, by a bug or by whoever has `engine.key`, fails it. The reasons are in [ADR-0070](../adr/0070-the-operators-signature-on-every-human-decision.md); this page is the procedure.

Two files, in the config directory beside `engine.key`:

| File | What | Mode | Keep a copy? |
| --- | --- | --- | --- |
| `operator.key` | Your approval key, sealed under your passphrase. | 0600, owned by you | Yes, with the passphrase kept apart from it. |
| `operator.pub` | Its public half. `hyalos log verify` reads it. | 0444 | Yes. It is not secret. |

## Set it up once

After `hyalos init`:

```
hyalos operator init
```

It asks for a passphrase twice, at the terminal with echo off. Use at least 12 characters, and one the driving agent has never seen: **the passphrase is what stops the agent deciding for you.** The agent may be able to read `operator.key`, as it may read `engine.key`; without the passphrase it cannot sign with it. Never type it into the agent's session, put it in a file, or pass it in the environment.

`hyalos operator show` prints the public key. Keep a copy of it off this machine, as you do for `engine.pub`.

The key is never replaced: `init` refuses if either file exists, since your past decisions name this key.

## Deciding

Each command asks for the passphrase before it writes anything, and writes nothing if it does not open the key.

```
hyalos approve <hold>        # release a hold (default reason approved.operator)
hyalos cancel <task>         # end a held task instead (default reason cancelled.operator)
hyalos release <task>        # release the issue a run waits for to its actor (default reason handle.operator_released)
hyalos policy approve <file>
hyalos agent issue <name>
hyalos agent revoke <id>
hyalos run ... --issue-number <n> --release-issue
hyalos merge <owner/repo> <pr> --head <commit>   # see Merging below
```

Each takes `--reason <code>` where it did before. A hold's id and a task's id are the numbers `hyalos log verify`, the MCP server's `job_status` and the run's own output show.

In a script you run yourself, `--passphrase-stdin` reads one line from standard input instead of the terminal. Do not give the agent a script that holds the passphrase.

The MCP server has no tool that decides, and never opens `operator.key`. A run the agent starts holds the issue and its actor waits for you ([ADR-0078](../adr/0078-agent-started-runs-wait-for-the-operator-to-release-the-issue.md)). The agent's `job_status` and `list_approvals` show which task waits and for which issue. Read the issue on GitHub, then run `hyalos release <task>`: the actor may then read the issue's text as its users wrote it. Or run `hyalos stop <task>` instead. If you do neither, the run ends at its deadline (`handle.unreleased`). The agent's credential is not a key and opens nothing.

## Merging

Hyalos merges a pull request only after you approve it ([ADR-0071](../adr/0071-merging-after-the-operators-approval.md)). Review the pull request on GitHub, note the full id of its head commit (the commits tab, or `git rev-parse` on the fetched branch), then:

```
hyalos merge owner/repo 42 --head 0123456789abcdef0123456789abcdef01234567 [--method merge|squash|rebase]
```

It prints the repository, number, head and method, asks for the passphrase, records your signed approval in a task of its own, and merges with a token minted for this merge and revoked after it. The default method is `merge` and the default reason `merge.operator` (`--reason <code>`). Exit 0 means GitHub merged it, and it prints the merge commit.

If anyone pushed after you reviewed, GitHub refuses (`merge.head_moved`) and nothing is merged: review the new commits and run it again with the new head. Other refusals are codes too, never GitHub's text: `merge.not_mergeable` (a conflict, a required check not passed, the method not allowed in the repository, or the pull request closed), `merge.rejected`, `merge.unconfirmed` (GitHub accepted the merge but its answer could not be read: check the pull request before running it again), `forge.upstream_refused` (the App may not merge here; see [the GitHub App setup](github-app.md#6-let-the-app-merge-optional)). Your operator policy must permit the `merge` action. The driving agent has no tool that merges.

## Checking

```
hyalos log verify
```

With `operator.pub` in the config directory it prints `human decisions: N signed by the operator, key …`. It fails if a human decision written by this version carries no signature, is signed by another key, does not hold for that event in this log, or names a place in the log it was not signed for (an approval written again later, or a log that differs from the one you saw). On another machine pass `--operator-key <hex>` or `--operator-key-file <path>`, as you pass `--public-key`.

Human decisions written before signed approvals are listed, each as `unsigned: event N (kind) ...`: they rest on the engine's key alone. Nothing more can be checked for them.

## If you lose the passphrase or the key file

No further decision of yours can be recorded on this log: every command above fails. There is no way to set up a new key in this version (ADR-0070 leaves rotation to its own decision); keep the key file backed up and the passphrase somewhere you will not lose it.
