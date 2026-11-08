# ADR-0077: Handing the operator one step at a time

**Status:** Accepted. Supersedes [ADR-0076](0076-the-set-up-hyalos-skill-and-the-setup-checklist.md).

## Context

ADR-0076 made `hyalos setup` the checklist and the `set-up-hyalos` skill the walk through it. The first timed run on a clean machine (#253, on bot1) found the steps correct but hard to follow (#258):
- The skill put many steps on screen at once, and the session kept scrolling as the builds and checks printed.
- Each operator step is a command the operator runs in their own terminal. The commands sat inside prose and tables, so copying them cleanly from one window to another was fiddly.
- Some steps had placeholders to edit, such as the App ID, the downloaded key's path, and `--harness claude-code` "at the end". Editing them by hand was error-prone, and having the agent fill them in put the agent's text into commands the operator runs.

ADR-0076's Decision also has the operator write the provider key while the builds run. That was before `hyalos` existed on the machine, so the agent had to give the commands from its own text.

## Decision

Everything in ADR-0076 stands, except as follows.

**Each step carries its commands.** Every step in `hyalos setup` has `commands`: the lines to paste into a shell, shown in `--json` too. `#` lines say what the step does. Commands that need something only the operator knows ask for it at the prompt, rather than leaving a placeholder:
- **Provider key:** one POSIX shell line. It asks which provider (`openai` or `anthropic`) and then the key, which it reads with echo off, so the key is never on a command line or in shell history. It writes `provider.keys` owner-only. It writes nothing for an empty answer or for a provider the file already names, so pasting it again after a mistake is safe.
- **The App:** one POSIX line. It asks for the App ID (digits only) and the full path of the downloaded `.pem`, moves the key into place owner-only, and writes `github.app`.
- **Agent credential:** one line, run in the repository's directory. It passes `hyalos agent issue`'s output, which is the credential alone, straight to `claude mcp add`, so the credential is never shown. It adds `--harness claude-code` when `provider.keys` has only Anthropic's key.

  The operator still types their passphrase into `agent issue` and `policy approve` themselves, at `/dev/tty`.

Paths are quoted for the shell. A config directory whose path is not UTF-8, or has a line break, is refused with a message to set `HYALOS_CONFIG_DIR`.

**`hyalos setup --next [--json]`** prints only the first step not done. It shows the detail and the document, then ends with the commands and nothing after them, so the block copies cleanly. If every step is done, it says so. With `--json` it prints `next` in place of `steps`. Exit codes are as for `hyalos setup`.

**The skill hands over one step at a time.** It gives only the step the operator takes now, as a one-line note and then one fenced block of that step's `commands`, exactly as given. It never edits them. It waits until the operator says the step is done before giving the next, and tells them once that `hyalos setup --next` shows the next step in their own terminal. It runs the builds in the background without streaming their output.

While the builds run, the operator does only the browser work: creating the App, and creating the provider key with a spend cap. The skill opens the App's prefilled page in their browser (`xdg-open`, or `wslview` or `explorer.exe` on WSL2) rather than printing the link, since on bot1 copying the wrapped link put a space in the homepage URL. The link names only what is not GitHub's default, with `https://github.com` as the homepage, so it is short if it must be printed. Writing the files waits until `hyalos` is installed and can print the commands.

## Consequences

The operator copies one short block at a time, or none: `hyalos setup --next` in their own terminal gives them the next step without the agent's window. The commands they paste come from Hyalos, not from the agent's text, and a mistake is fixed by pasting the line again.

Writing the files after the install moves those two steps off the builds' critical path only partly. The browser work, which takes longer, still overlaps the builds.

The one-line commands are long and need a POSIX shell, which every supported host has. A test pastes the provider key's line into `sh` with an empty key, a good key and a repeat.
