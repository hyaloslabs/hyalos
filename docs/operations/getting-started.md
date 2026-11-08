# Getting started

How to go from a clone to a first contained job. The fastest way is to ask your agent: open Claude Code in the repository and say "set up Hyalos". The `set-up-hyalos` skill ([`skills/set-up-hyalos/SKILL.md`](../../skills/set-up-hyalos/SKILL.md), [ADR-0076](../adr/0076-the-set-up-hyalos-skill-and-the-setup-checklist.md)) walks you both through it. For another client, copy that directory into the client's skills directory.

You keep every secret and every decision. The agent never needs your passphrase, a provider key or the App's private key. Once `hyalos` is installed, run `hyalos setup walk` in a terminal of your own, not through the agent. It takes each step that is still missing, asks for what the step needs, writes it owner-only and checks it. You copy nothing from the agent's window ([ADR-0080](../adr/0080-setup-asks-the-operator-in-their-own-terminal.md)).

## By hand

1. Install the binary from the repository root: `cargo install --locked --path core/hyalos`. Build the image at the same time: `docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .` (name the target: the last stage is the test image).
2. Run `hyalos setup`. It lists what is missing, in order, who takes each step and where it is written down, and writes nothing:

   ```
   setup: 4 of 7 steps done
   config directory: /home/you/.config/hyalos
     [done] engine_key: engine.key, engine.pub and the event log check out
     [todo] operator_key (operator): no usable operator key: ...
            do: in a terminal of your own: hyalos setup operator-key
            see: docs/operations/approvals.md
     ...
   ```

   It exits 0 once every step is done. `--json` prints the same as JSON, and `--next` only the first step not done. Your files go in the config directory it names.
3. In a terminal of your own, run `hyalos setup walk`. It runs `hyalos init` if the engine key is missing, then takes each of your steps that is not done. Each step is also a command of its own, safe to run again, which asks before it replaces anything:

   | Command | What it asks and does |
   | --- | --- |
   | `hyalos setup operator-key` | A passphrase of 12 or more characters, twice. It makes the approval key that signs your decisions. |
   | `hyalos setup provider-key` | `openai` or `anthropic`, then the key, not shown. It writes that provider's line in `provider.keys`, owner-only. |
   | `hyalos setup github-app [<pem>]` | It says what to do in a browser on any computer, then asks for the App ID. It takes the key from `<pem>`, or finds GitHub's download (`*.private-key.pem`) in the config directory, your home directory, `~/Downloads` or, on WSL2, Windows' downloads, and copies it to `github-app.pem` owner-only. On a machine you reach over SSH, it prints the `scp` line that copies the key there. |
   | `hyalos setup policy` | It prints the starter policy, which allows a first job and nothing more: read, push the job's branch, open a pull request, with data going only to the model and GitHub. It asks before you approve it with your passphrase. For another policy, use `hyalos policy approve <file>`. |
   | `hyalos setup agent` | Run it in the directory you start Claude Code in. It issues a credential and registers the `hyalos` server with Claude Code, without showing the credential. `--harness claude-code` makes the server's jobs use Claude Code; it is added for you when your only provider key is Anthropic's. |

   A step that fails says what was not written. Fix that, and run the command again.
4. Check the App can reach the repository with `hyalos github check <owner/repo>`, then start a job with `hyalos start <owner/repo> <issue-number>` ([mcp.md](mcp.md)) or `hyalos run` ([running-a-job.md](running-a-job.md)).
