# ADR-0060: The workflow document, and the check at install

**Status:** Proposed

## Context

[ADR-0005](0005-provenance-is-a-type.md) gives workflow steps four roles:
- **reader:** no tools, emits typed values;
- **planner:** typed values in, typed values out;
- **actor:** typed inputs, raw text only through a gated handle;
- **deterministic.**

It also says the engine type-checks a workflow at install, so that letting tainted text reach an actor's prompt is an install error. S4.2 (#24) asks for the first piece: a workflow format where an author declares step roles and typed reader schemas, and which rejects an actor step that names raw text as input.

No workflow format exists. `hyalos run` runs one actor with no workflow at all ([ADR-0050](0050-running-one-job-from-the-command-line.md)), which [ADR-0059](0059-tainted-text-cannot-reach-an-actor-prompt-by-type.md) keeps as a named exception until the fix-issue workflow (S4.6, #78) replaces it. ADR-0059 also fixed the typed kinds a value may take, and ruled out any kind that can carry free text. [ADR-0016](0016-agent-runtime-and-broker-protocols.md) says a step's typed result has no unbounded free-text fields, and that a string derived from tainted input reaches an actor only through a gated handle.

## Decision

**A workflow is a JSON document** (`hyalos_types::workflow`), read with `Workflow::parse`. JSON because every other document the engine reads is JSON, and `hyalos-types` already depends on `serde_json`, so no new dependency enters a deterministic crate.

The document has:
- a `version` (1; any other is refused, never guessed);
- a `name`;
- `inputs`, each a name and a kind;
- `steps`, in order.

Every object refuses unknown fields. A step has:
- an `id`;
- a `role`;
- `reads`, a list of references;
- `instructions`, the author's text;
- for a reader or a planner, an `output` of named typed fields.

```json
{
  "version": 1,
  "name": "fix_issue",
  "inputs": { "issue": "forge_issue", "task": "operator_text" },
  "steps": [
    { "id": "triage", "role": "reader", "reads": ["issue"], "instructions": "Classify the issue.",
      "output": { "kind": { "choice": ["bug", "feature", "docs"] },
                  "severity": { "integer": { "min": 1, "max": 5 } } } },
    { "id": "fix", "role": "actor", "reads": ["task", "triage.kind"],
      "instructions": "Make the change and open a pull request." }
  ]
}
```

**Input kinds are closed.** `forge_issue` is raw forge-authored text (an issue as [ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md) reads it). `operator_text` is raw text the operator gives when the workflow runs. A new kind is a new version of this module.

**Output fields are ADR-0059's typed kinds and nothing else:**
- `bool`;
- `{"integer": {"min", "max"}}`, with `min <= max`;
- `{"choice": [...]}`, with 1 to 64 distinct, non-empty options of at most 128 bytes, each written by the workflow's author.

There is no string field. A schema cannot declare free text, so no reader's output can carry it. What a reader would need to pass on as text goes through a gated handle (S4.4, #26).

**The check at install.** A document that fails any of these does not install, and each failure has a short code:

| Check | Code |
| --- | --- |
| The document is at most 64 KiB. | `workflow.too_large` |
| It is JSON of this shape, with no unknown field, no unknown kind and no key named twice in any object. | `workflow.malformed` |
| Its version is 1. | `workflow.unknown_version` |
| Names (workflow, inputs, steps, fields) are 1 to 32 of `a-z`, `0-9` and `_`, starting with a letter. | `workflow.bad_name` |
| A step id is unique and is not also an input's name. | `workflow.duplicate_name` |
| There are 1 to 32 steps, at most 16 inputs, 32 reads per step and 16 fields per output. | `workflow.no_steps`, `workflow.too_many` |
| Instructions are at most 16 KiB. | `workflow.instructions_too_long` |
| Every reference names an input, or `step.field` of an earlier step. A whole step is not a value. | `workflow.unknown_reference`, `workflow.forward_reference` |
| A reader and a planner have at least one output field. | `workflow.output_required` |
| Each field is well formed. | `workflow.bad_field` |
| A deterministic step is refused: nothing defines what one runs yet. | `workflow.role_not_supported` |
| **An actor step reads no raw tainted text.** | **`workflow.raw_text_to_actor`** |
| **A planner step reads no raw tainted text.** | **`workflow.raw_text_to_planner`** |

Raw tainted text is an input whose kind taints, which today is `forge_issue`. Only a reader reads it. Operator text may go to any step, as it may go into an `ActorPrompt`.

**The check derives what each step reads** (`Checked::inputs`), by provenance:
- A step reads `Operator` always, since its instructions are the author's.
- An input adds its kind's provenance.
- A field adds the provenance of what its step made. A step that runs tainted (`StepInputs::tainted(role)`) makes `ModelFromTainted`; one that does not makes `Operator`, since a model that read only operator text chose among options the operator wrote.

So taint follows the issue through every step: a reader of the issue taints its planner, and the planner taints the actor. Those `StepInputs` are what each step's manifest is minted with. The runner that mints them comes with S4.6.

**`hyalos workflow check <file>`** reads a document, checks it, writes nothing, and prints each step's role, what it reads and whether it runs tainted. Exit 0 means it installs, 1 that it does not (with the code), 2 is usage. There is no install command yet: storing an installed workflow comes with the first workflow that runs (S4.6).

## Consequences

S4.2's acceptance holds: an actor step that names raw text as input fails the check with `workflow.raw_text_to_actor`. This is the "fails to install" of the PRD, applied to the document before anything runs.

The step's inputs now have a source other than code that writes them by hand. `StepInputs` can still be built by hand and passed to `mint_manifest`. Closing that path means a runner that mints only from `Checked`, which is S4.6.

What this does not decide:
- **How a reader's output is validated** against its schema, and in which process. That is S4.3 (#25): the broker validates, and the engine receives only typed values.
- **How a planner or actor submits a result.** That is S4.7 (#84, `hyalos-submit`). An actor's `output` is allowed by the format and not yet used.
- **Gated handles** (S4.4) and **plugin-authored instructions** (S4.8): `instructions` are the author's, the operator's today.
- **Which tools, egress hosts and budget a step gets.** These stay in the manifest that `hyalos run` mints. The format will grow them with S4.6, as a new version if the shape changes.

A key named twice is refused anywhere in the document, including in `inputs` and `output`, where `serde_json` would otherwise keep the last value. What a reviewer reads first is then what was checked. A schema author who needs text after all has to choose among options or wait for handles, which is the restriction ADR-0059 chose.
