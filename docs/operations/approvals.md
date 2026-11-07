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
hyalos policy approve <file>
hyalos agent issue <name>
hyalos agent revoke <id>
hyalos run ... --issue-number <n> --release-issue
```

Each takes `--reason <code>` where it did before. A hold's id and a task's id are the numbers `hyalos log verify`, the MCP server's `job_status` and the run's own output show.

In a script you run yourself, `--passphrase-stdin` reads one line from standard input instead of the terminal. Do not give the agent a script that holds the passphrase.

The MCP server has no tool that decides, never opens `operator.key`, and the runs the agent starts cannot release an issue's text. The agent's credential is not a key and opens nothing.

## Checking

```
hyalos log verify
```

With `operator.pub` in the config directory it prints `human decisions: N signed by the operator, key …`. It fails if a human decision written by this version carries no signature, is signed by another key, does not hold for that event in this log, or names a place in the log it was not signed for (an approval written again later, or a log that differs from the one you saw). On another machine pass `--operator-key <hex>` or `--operator-key-file <path>`, as you pass `--public-key`.

Human decisions written before signed approvals are listed, each as `unsigned: event N (kind) ...`: they rest on the engine's key alone. Nothing more can be checked for them.

## If you lose the passphrase or the key file

No further decision of yours can be recorded on this log: every command above fails. There is no way to set up a new key in this version (ADR-0070 leaves rotation to its own decision); keep the key file backed up and the passphrase somewhere you will not lose it.
