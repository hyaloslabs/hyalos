# ADR-0102: Composing skills, components and deterministic steps in a workflow

**Status:** Accepted

## Context

S6.5 (#40) asks that a workflow author compose skills, components and deterministic steps, and that a workflow mixing all three install and pass the role checks of E4.

What existed:
- [ADR-0060](0060-the-workflow-document-and-the-check-at-install.md) defines the workflow document and its check. It refuses a deterministic step (`workflow.role_not_supported`), since nothing defined what one runs.
- [ADR-0087](0087-plugins-run-in-the-broker-with-capabilities-as-host-imports.md) hosts a plugin's WASM component in the broker, with its capabilities as host imports, but nothing ran one.
- [ADR-0089](0089-installing-an-agent-skill-and-running-it-in-the-job-s-sandbox.md) installs a skill and mounts it in the actor's sandbox (`hyalos run --skill`).
- [ADR-0066](0066-plugin-authored-text-and-the-grant-that-bounds-it.md) bounds a job that plugin text steers by the plugin's grant.
- `hyalos run` runs one actor, or the shipped fix-issue workflow ([ADR-0062](0062-the-fix-issue-workflow-and-its-runner.md)). An operator could check a workflow of their own (`hyalos workflow check`) but not run it.

The operator chose for a deterministic step to run an installed plugin's component.

## Decision

**Workflow version 2 lets a step name a plugin.** A step may have `plugin`, an installed plugin's name. Version 1 documents read and check as before. In version 2:
- A **deterministic** step must name one (`workflow.plugin_required`). It runs that plugin's component, and has an `output` of typed fields like a reader's (`workflow.output_required`).
- An **actor** may name one. It is the skill the actor is given, mounted in its sandbox as with `--skill`.
- A **reader** or **planner** names none, and a version 1 document names none (`workflow.bad_plugin`).
- A deterministic step reads no raw forge text (`workflow.raw_text_to_deterministic`), as an actor and a planner do not. It runs in the broker and is given typed values and operator text only.

**A plugin bounds every step it reaches.** The check derives, for each step, the plugins whose grants bound it (`Checked::plugins`): the step's own, and every plugin whose step's fields it reads, through any number of steps. A step reached by a plugin reads `PluginAuthored`, so its manifest is minted within those plugins' grants (ADR-0066). A field a deterministic step makes is `PluginAuthored`, or `ModelFromTainted` if the step ran tainted, so taint still follows the issue. `hyalos workflow check` prints the plugins that bound each step.

**A deterministic step is a component run with no capability.** The component is the plugin's `component.wasm`, installed with it as any file of a skill is, and pinned by its digest. The engine starts the step as a job of role `deterministic`, with no forge action, no egress and no data flow, and sends `RunPlugin { job, call }` (wire version 10). The call holds the plugin's name and digest, the plugin store, the step's input and its output. The engine never sends the component's bytes. The input is a JSON object: the step's instructions, and each value it reads by reference (`{"instructions": "...", "reads": {"task": "...", "triage.kind": "bug"}}`), at most 32 KiB.

The broker:
1. Runs it only for a live job whose manifest's role is `deterministic` (`plugin.wrong_role`), with an input and output within their limits (`plugin.bad_call`).
2. Reads the plugin from the store and checks it against the digest (`plugin.altered`).
3. Instantiates the component with no capability linked, so a component that imports one does not load (`plugin.undeclared_import`). A deterministic step is a function of its input. ADR-0087's limits on memory, fuel and calls apply.
4. Reads its answer as the step's output: exactly the object a reader answers with, with no fence (`plugin.answer_not_json`, `plugin.answer_does_not_fit`, `plugin.answer_too_large`). The plugin's own error is `plugin.failed`, and its text is never kept.

The answer reaches the engine as typed values only. The engine checks them against its own copy of the output. The `hyalos-broker` crate still links no WASM host ([ADR-0086](0086-the-plugin-runtime-s-crate-and-line-budget.md)). It takes a `Plugins` implementation from the broker process's own code in the `hyalos` crate, which runs the step with `hyalos_plugin::run_step`.

**`hyalos run --workflow <file>` runs the operator's workflow** instead of the fix-issue workflow. The issue comes from `--issue-number` and the operator's text from `--issue`. Every plugin the workflow names is loaded and checked against its install before anything is written (`plugin.not_installed`, `plugin.altered`, `plugin.revoked`). The document is stored with the run request, left out when absent so older requests keep their digest. A run with `--workflow` is not resumed.

## Consequences

S6.5's acceptance holds. A workflow of a reader of the issue, a deterministic step running a component, a planner reading its answer, and an actor given a skill passes the check. Every role check of E4 still refuses raw forge text given to the actor, the planner and the deterministic step.

A plugin's output never widens a job. A step that reads a component's answer, however indirectly, is minted within that component's grant. So a component granted nothing leaves the actor it steers no forge action.

Deterministic steps get no capability in this version. A component that needs the network, a tool or the forge needs host implementations in the broker for ADR-0087's interfaces, and a decision on what a deterministic step may reach. That is later work.

`plugin_text` inputs still cannot be supplied by `hyalos run`. An actor takes a skill by naming it as its `plugin`.

Costs: 385 lines of trusted core:
- the document, check, wire message and answer reader in `hyalos-types` (+94);
- the broker's `RunPlugin` and its `Plugins` hook in `hyalos-broker` (+53);
- `run_step` in `hyalos-plugin` (+39);
- the runner, `--workflow`, the broker process's runtime and `workflow check` in `hyalos` (+199).

Trusted core goes from 27,632 to 28,017 lines of 30,000.

Tests:
- `core/types/src/workflow.rs`: the composed workflow installs; its plugins bound the steps they reach; the role checks refuse raw forge text to the actor, the planner and the deterministic step. A deterministic step needs a plugin and an output, and only version 2 names one. A plugin's answer is read as typed values only.
- `core/types/src/wire.rs`: `RunPlugin`'s pinned encoding.
- `core/broker/tests/serve.rs`: a plugin runs only for a live deterministic job, with a call within its limits, in a broker with a runtime.
- `core/plugin/tests/host.rs`: `run_step` runs a component with no capability. A component importing one does not load, and an answer that does not fit, the plugin's error and a plugin with no component are codes.
- `core/hyalos/tests/plugin.rs`: `hyalos run --workflow` runs the step's component in the broker and records its value before the actor runs. An actor naming the plugin is given it as a skill. An answer that does not fit fails the run before the actor. A component granted nothing leaves the actor no forge token. A reader, a component reading the reader's value, a planner and an actor run as one workflow: the component's tainted job calls no model, and neither the planner nor the actor gets the issue's text.
