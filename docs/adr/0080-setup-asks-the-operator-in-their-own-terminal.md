# ADR-0080: Setup asks the operator in their own terminal

**Status:** Accepted. Supersedes [ADR-0077](0077-handing-the-operator-one-step-at-a-time.md). Its account of `github-app` is superseded by [ADR-0084](0084-setup-creates-the-github-app-from-a-manifest.md), for an App not set up yet.

## Context

ADR-0077 gave each operator step its commands: long POSIX one-liners, and a long prefilled link for the GitHub App, to copy from the agent's window into the operator's terminal or browser. The second timed run on bot1 (#253) showed that copying a long line breaks it. A terminal turns a wrapped line's wrap into a line break or spaces when it is copied:
- the provider key's line failed with a shell syntax error;
- the App link's homepage became `https://github  .com`, for the second run in a row;
- `xdg-open` cannot open a browser on a machine reached over SSH.

The operator also hit these:
- Nobody said how to get the App's `.pem` onto a headless machine. They copied it into the config directory, where the App's line then failed, because it could not move the file onto itself (#274).
- `less policy.cedar` held them until Ctrl-Z.
- The operator's shell had no `cargo` or `hyalos` on its PATH, because the rustup line lost its last part when copied.

## Decision

**Every operator step is a short `hyalos` command that asks for what it needs in the operator's terminal, writes it owner-only and checks it.** No step asks the operator to copy a line longer than a terminal row, or a link.

```
hyalos setup walk                  every step still missing, in order
hyalos setup operator-key          the approval key and its passphrase
hyalos setup provider-key          a provider's API key
hyalos setup github-app [<pem>]    the App's id and private key
hyalos setup policy                the starter policy, shown and approved
hyalos setup agent [--harness <h>] [--dir <dir>]
                                   a credential, registered with Claude Code
```

`hyalos setup` with no subcommand, `--next` and `--json` stay the read-only checklist the agent reads (ADR-0076). Each step's `commands` is now the one short command that takes it.

**They show and ask only at the terminal.** What they show, the policy to be signed included, is written to `/dev/tty`, where the questions are asked; only the final summary goes to standard output. Answers and secrets are read from `/dev/tty`, a secret with echo off, so none is ever in an argument, the environment or shell history. Without a terminal they refuse before showing or writing anything, and the end of input is a refusal, never a default answer. As with the passphrase (ADR-0070), that is not what keeps the agent out: the agent must not run them, and what stops it deciding is that it does not know the passphrase. The functions under them take their answers from a trait, which the tests script.

**Each is safe to run again.** A step already done says so. Run on its own, it asks before it replaces what is there. Run by `walk`, a done step is skipped. Nothing is written until every answer is in and checked, and a failure says in one sentence what was not written.

**What each one does:**
- **`walk`** runs `hyalos init` itself if the engine key is missing, then each operator step that is not done. A sandbox image that is not ready yet does not stop it, since no other step needs the image; it says so at the end. It asks for the passphrase once, for all the decisions it makes.
- **`provider-key`** asks which provider and the key. It replaces that provider's `key` line in `provider.keys`, keeping every other line, and writes the file owner-only through a temporary file and a rename.
- **`github-app`** prints what to do in a browser, as five short lines. When it is a terminal, the address it prints (`github.com/settings/apps/new`) is a terminal hyperlink (OSC 8) to the prefilled page, so a click opens it with the name and permissions set. A terminal without hyperlinks shows the address alone, and the five lines say what to set. Then it asks for the App ID. It takes the key from `<pem>`, or finds the newest file that can only be the App's key (GitHub's download name, `*.private-key.pem`, or `github-app.pem`) in the config directory, the home directory, `~/Downloads` or WSL's Windows downloads, and asks before using it. Any other `.pem` the operator names themselves. If it finds none, it prints a short `scp` line naming this user and host, and looks again when the operator presses Enter. It checks the key is an RSA private key before writing anything, then copies it to `github-app.pem` owner-only. It removes the source only when it has GitHub's download name. A `github.app` it cannot read is refused, never overwritten, and its other lines are kept. A key already at `github-app.pem` is kept and its mode set to 600 (#274).
- **`policy`** prints the starter policy to the terminal, with no pager, and asks before approving it. The operator signs exactly what was printed. Any other policy is still approved with `hyalos policy approve <file>`.
- **`agent`** checks that `claude` runs, shows the directory the server is registered for (the current one, or `--dir`), and asks. It issues a credential named `claude-code`, replacing one in force only if the operator says so, and passes it to `claude mcp add` as an argument, as ADR-0077's line did. It never prints the credential or `claude`'s output. The server's command is this `hyalos` binary's full path, with `HYALOS_CONFIG_DIR` and `HYALOS_DATA_DIR` passed on when they are set. If `claude mcp add` fails, it revokes the credential it just issued.

**Decisions.** `setup walk`, `setup policy` and `setup agent` record decisions signed with the operator's key, so they join `tools_cmd::DECISIONS`. The agent never runs them, or the other operator steps, which take secrets.

**The skill** tells the operator to run `hyalos setup walk` in their own terminal and waits. It stops opening the App's page with `xdg-open`, since `hyalos setup github-app` now gives the link. The GitHub App manifest flow, which would create the App and hand back its key in one exchange, stays with the spike (#267).

## Consequences

The operator types a few words and answers questions. Nothing they copy can be broken by a wrap, and a wrong answer is fixed by running the command again.

A terminal without OSC 8 hyperlinks leaves the operator typing the App's address and setting four fields by hand, which is slower than the prefilled page. Most current terminals support them: Windows Terminal, GNOME Terminal, iTerm2 and kitty. The manifest flow would remove that work.

`hyalos setup agent` works only with Claude Code. Another client still gets a credential from `hyalos agent issue <name>`.

The credential is on `claude mcp add`'s command line for as long as that runs, as it was before. Another process of the same user can read it there, and that user can already read Claude Code's configuration file, which holds it.

The pasted shell lines and their test are gone. Each subcommand is tested through the trait, with scripted answers, a fake `claude`, and a key in each of the places it can land.
