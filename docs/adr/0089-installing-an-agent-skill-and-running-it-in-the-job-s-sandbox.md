# ADR-0089: Installing an Agent Skill and running it in the job's sandbox

**Status:** Accepted

## Context

[ADR-0006](0006-plugins-are-skills-and-wasm-components.md) makes an Agent Skill one of the two kinds of plugin: a `SKILL.md` and the scripts beside it, read by the harness and run in the job's sandbox. A skill brings no code into the engine or the broker, but its text steers the agent and its scripts run with whatever the job may do. [ADR-0012](0012-humans-approve-only-irreversible-actions.md) makes granting a capability a human's decision, [ADR-0085](0085-the-capability-vocabulary.md) gives the vocabulary for one, [ADR-0066](0066-plugin-authored-text-and-the-grant-that-bounds-it.md) bounds a job steered by plugin text to that plugin's grant, and [ADR-0088](0088-the-capability-diff-an-operator-reads.md) gives the diff an operator reads first.

S6.2 (#37) asks for a local install of a skill, and for a job that is given one to have it in its sandbox. Nothing recorded an install yet, nothing kept the files, and no substrate put anything but the job's own files in the sandbox.

## Decision

**A skill is a directory.** `hyalos_plugin::skill::Skill::read` reads `SKILL.md`, whose front matter names the skill (`PluginName`: 1 to 64 of `[a-z0-9-]`, no leading, trailing or doubled hyphen), the files beside it, and `capabilities.json` if it asks for any (an ADR-0085 document; none means it asks for nothing). It refuses a link, a name outside `[A-Za-z0-9_][A-Za-z0-9._-]*`, more than 256 files, 8 MiB or depth 8. The skill's digest is SHA-256 over a domain tag and, for each file in name order, its path, whether it is executable, and its bytes.

**Installing is the operator's decision.** `hyalos plugin install <dir> [--grant] [--reason <code>]` prints the skill's name, file count and digest, then the ADR-0088 diff of what it asks for against what that name holds now (nothing on a first install). If the diff adds a capability, it stops unless `--grant` was given, so the operator grants only after reading. It then copies the files into the plugin store (`plugins/` in the data directory, one read-only directory per digest, written beside and renamed into place), and writes `plugin.installed { name, digest, grant, source, by, reason, signed }` through `Engine::install_plugin`. As every decision ([ADR-0025](0025-releasing-a-hold-stakes-authority-and-evidence.md), [ADR-0070](0070-the-operators-signature-on-every-human-decision.md)), it is written only by that method, only by a human (installing grants capabilities and lets plugin text steer a job, so no policy or verifier may do it, even with an empty grant), signed with the operator's key, and refused raw from `submit` and from the broker. `source` is `local`; [ADR-0006](0006-plugins-are-skills-and-wasm-components.md)'s marketplace adds its own. The grant is checked against the vocabulary on write and on replay. A later install of the same name replaces the earlier one. `hyalos plugin install` joins `tools_cmd::DECISIONS`; there is no MCP tool for it. `hyalos plugin list` shows each installed skill, its digest, its event and its grant.

This is schema version 15 (the new event, no payload changed) and `StateImage` version 10 (`plugins`, one `PluginImage` per installed name), so compaction keeps installs and `hyalos log verify` checks their signatures.

**A job is given a skill by name.** `hyalos run --skill <name>` (repeatable; stored in the run request, left out when empty so older requests keep their digest) loads each skill before anything is written: the state's install for that name, then the store's files for its digest, read back and hashed again. A name not installed, or files that no longer hash to the install, refuse the run (`plugin.not_installed`, `plugin.altered`). The job's `StepInputs` take each skill's grant, so the manifest is cut to what the job and every skill it is given allow (ADR-0066). The prompt says only where the skills are, never their text.

**The files are mounted read-only.** The run writes the skills under the job's directory and sets `SandboxSpec::skills`. The container and gVisor drivers bind it read-only at `/opt/hyalos/skills`. `hyalos-forward` links `$HOME/.claude/skills` and `$CODEX_HOME/skills` to it, so each harness finds them where it looks. The microVM driver, which has one root filesystem drive, refuses a spec with skills (`SpecError::SkillsUnsupported`), and the run ends the job as `sandbox.not_started` rather than run it without them. The substrate contract has a check for this: skills are readable and not writable at their place, or the spec is refused.

## Consequences

- A skill is pinned: the files a job gets are the files the operator installed, by digest, and a change to them in the store refuses the run.
- A skill never widens a job. Giving a job a skill that was granted nothing leaves it able to do nothing, which is the point: the operator grants what the skill needs.
- A skill's scripts run in the sandbox under the job's own limits. They are not sandboxed from the agent, and need not be: they hold nothing the agent could not already do.
- Jobs that use skills cannot run on the microVM substrate yet. Carrying them needs a second drive or files written into the root image, which is later work.
- Install from a marketplace (S7.9), install policy (S7.5) and update checks (S7.10) build on this event and store.
