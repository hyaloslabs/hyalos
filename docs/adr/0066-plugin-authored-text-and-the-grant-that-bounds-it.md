# ADR-0066: Plugin-authored text, and the grant that bounds the job it steers

**Status:** Proposed

## Context

S4.8 (#88) follows from decision D5 of the reference-job walk. That decision is recorded in [ADR-0005](0005-provenance-is-a-type.md): plugin-authored text is its own provenance class. It may enter an actor's prompt, so an installed skill can instruct an actor. It never widens the job: what the actor can do is the intersection of the plugin's granted capabilities and the job's manifest ([ADR-0006](0006-plugins-are-skills-and-wasm-components.md)).

The acceptance:
- Plugin-authored text can enter an actor's prompt, and the install check still rejects tainted raw text reaching an actor.
- A skill that asks for broader access than the job manifest gets only the intersection of its capabilities and the manifest.
- A skill cannot widen a job's manifest.

What exists:
- `Provenance::PluginAuthored`, which does not taint.
- `PluginText`, constructible only in tests.
- `ActorPrompt::plugin` ([ADR-0059](0059-tainted-text-cannot-reach-an-actor-prompt-by-type.md)).

Nothing tied plugin text to what its plugin was granted, so nothing could bound a job by it. The workflow language ([ADR-0060](0060-the-workflow-document-and-the-check-at-install.md)) had no input a skill's instructions could come in by.

Granting capabilities to a plugin is E6's: local install with zero capabilities and a human approval for each grant ([ADR-0006](0006-plugins-are-skills-and-wasm-components.md), [ADR-0012](0012-humans-approve-only-irreversible-actions.md)). So is the capability vocabulary shared by manifests and plugins (S6.6, #82). Neither is built.

## Decision

**A grant is written in the manifest's terms.** `hyalos_types::plugin::Grant` holds the capabilities granted to one plugin as the manifest's own sets: `actions`, `egress_hosts`, `tools` and `data_flows`. It uses `deny_unknown_fields`, and `Grant::none()` is what a plugin has before a human grants anything. Two operations:
- `Grant::bound(&manifest)` returns the manifest with each of those sets cut to the intersection and everything else unchanged. The result can never be wider than the manifest.
- `Grant::allows(&manifest)` says whether every set is within the grant.

S6.6 will define and version the vocabulary. Until then, a grant can name only what a manifest can.

**Plugin text carries its grant.** `PluginText::granted(text, grant)` is how plugin text is made. Its maker is whatever loads the plugin, from what was installed and what was granted. Adding plugin text to a prompt (`ActorPrompt::plugin`, `ReaderPrompt::plugin`) records the grant. `inputs()` returns `StepInputs` that now hold `grants` beside `reads`, and `StepInputs::with_grants` adds them, marking the inputs plugin-authored.

**Inputs bound a manifest.** `StepInputs::bound(&manifest)` cuts the manifest by every grant in turn. With two skills in one prompt, the job is within both. `StepInputs::within_grants(&manifest)` checks the same thing without changing anything. Plugin-authored input with no grant to go with it, such as a typed value sourced `PluginAuthored`, is bounded by `Grant::none()`, so it fails closed.

**The mint refuses a plugin widening a job.** `Engine::mint_manifest` refuses a manifest that is not within its inputs' grants, with `mint.plugin_widens`, after the taint check. So the bound is the engine's, not only the caller's: a caller that forgets to cut the manifest gets no manifest, never a wider one.

**A workflow can take a skill's instructions.** `InputKind::PluginText` (`"plugin_text"`) is raw text with plugin-authored provenance. It does not taint, so the install check lets an actor or a planner read it. The check still refuses forge text given to an actor or planner (`workflow.raw_text_to_actor`, `workflow.raw_text_to_planner`), whatever else the step reads.

**The runner puts it in the actor's prompt and bounds the job.**
- `workflow_run::Supplied.plugins` maps each `plugin_text` input to its `PluginText`. One not supplied is `workflow.input_missing`.
- The actor's prompt gives the text after the task, marked as an installed skill's instructions.
- `Runnable::actor_inputs` is the checked inputs plus the grants from the prompt.
- `hyalos run` cuts the actor's manifest with `inputs.bound` before it mints the manifest and the token.

Readers and planners run in the broker, which gets only operator text and typed values ([ADR-0061](0061-reader-steps-run-in-the-broker.md)). So this runner refuses a reader or planner that reads plugin text (`workflow.plugin_text_in_broker`), though the check installs it.

**`hyalos run` supplies no plugin text yet.** There is no installed plugin to load it from until E6. The path is built, and tested with plugin text made in the test.

## Consequences

S4.8's acceptance holds:
- **Plugin text reaches the actor; tainted text still doesn't.** An actor reading a `plugin_text` input installs, its inputs are plugin-authored, and the runner gives it the text. The same workflow with the issue given to the actor does not install.
- **A broader skill gets only the intersection.** A skill granted `push`, `publish` and `change_policy` on a job that has `open_pr` and `push` gets `push` only. A manifest wider than the grant is not minted (`mint.plugin_widens`).
- **No skill widens a job.** Bounding by a grant that holds everything leaves the manifest as it was. Two skills narrow it to what both hold. Plugin-authored values with no grant allow nothing.

The tests:
- `core/types/src/plugin.rs`: the intersection, never widening, no grant, and unknown fields.
- `core/types/src/workflow.rs`: plugin text reaches an actor and a planner untainted, and the issue still does not reach the actor.
- `core/engine/tests/policy_store.rs`: minting through a real `ActorPrompt`, for a broader skill, a skill that holds everything, two skills, and an ungranted plugin value.
- `core/hyalos/src/workflow_run.rs`: the runner supplies the skill, puts it in the prompt, carries its grant, and bounds the manifest. A reader or planner that reads a skill does not run here.

**What this does not decide:**
- **Where a grant comes from.** That is the install and consent of E6 (S6.4, S6.5, S6.7). Nothing yet writes a grant to the log, and `PluginText::granted` trusts its caller as `OperatorText::new` does.
- **The capability vocabulary and its versioning** (S6.6). A grant uses the manifest's sets, so a manifest change changes the grant too.
- **Plugin text for readers and planners.** That would need the broker to take plugin text on the channel.
- **Budget, deadline and repository.** A grant does not bound these. A plugin cannot raise them either, since only the engine sets them.

**Costs.** 118 more lines of trusted core:
- the grant, the inputs' grants and bound, the prompt's grants and the input kind (+91, types);
- the runner's plugin input and bounded manifest (+21, hyalos);
- the mint check (+6, engine).

Trusted core goes from 16,381 to 16,499 lines of 30,000.
