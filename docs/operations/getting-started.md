# Getting started

How to go from a clone to a first contained job. The fastest way is to ask your agent: open Claude Code in the repository and say "set up Hyalos". The `set-up-hyalos` skill ([`skills/set-up-hyalos/SKILL.md`](../../skills/set-up-hyalos/SKILL.md), [ADR-0076](../adr/0076-the-set-up-hyalos-skill-and-the-setup-checklist.md)) walks you both through it. For another client, copy that directory into the client's skills directory.

You keep every secret and every decision. The agent never needs your passphrase, a provider key or the App's private key. Run the steps it hands you in a terminal of your own, not through the agent.

## By hand

1. Install the binary from the repository root: `cargo install --locked --path core/hyalos`. Build the image at the same time: `docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .` (name the target: the last stage is the test image).
2. Run `hyalos setup`. It lists what is missing, in order, who takes each step and where it is written down, and writes nothing:

   ```
   setup: 4 of 7 steps done
   config directory: /home/you/.config/hyalos
     [done] engine_key: engine.key, engine.pub and the event log check out
     [todo] operator_key (operator): no usable operator key: ...
            do: in a terminal of your own: hyalos operator init
            see: docs/operations/approvals.md
     ...
   ```

   It exits 0 once every step is done. `--json` prints the same as JSON. Your files go in the config directory it names.
3. For the policy, run `hyalos policy starter > policy.cedar` yourself; it prints one that allows a first job and nothing more: read, push the job's branch, open a pull request, with data going only to the model and GitHub. Read it, then approve it with `hyalos policy approve policy.cedar`. Do this step yourself rather than through the agent, so the file you sign is the one you read.
4. Check the App can reach the repository with `hyalos github check <owner/repo>`, then start a job with `hyalos start <owner/repo> <issue-number>` ([mcp.md](mcp.md)) or `hyalos run` ([running-a-job.md](running-a-job.md)).
