# ADR-0059: Tainted text cannot reach an actor's prompt, by type

**Status:** Proposed

## Context

[ADR-0005](0005-provenance-is-a-type.md) says every value carries its provenance and that letting tainted text reach an actor's prompt is an error. S4.1 (#23) makes this concrete: code that passes tainted text to an actor's prompt fails to compile.

What exists ([ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md)):
- `Provenance` has four classes, and `StepInputs::tainted(role)` derives taint from them.
- `ForgeText` is forge-authored text, and `ForgeIssue` holds an issue as `ForgeText` fields.

What does not hold yet:
- **The text is one call away.** `ForgeText::as_str` is public. Any code can turn an issue's body into a `&str` and put it anywhere, including an actor's prompt.
- **The provenance is written by hand.** `run_cmd` picks `StepInputs` from whether `--issue-number` was given, not from what the prompt actually holds. A later change could put forge text in a prompt and leave the inputs saying `Operator`.
- **There is no type for a prompt.** Prompts are `String`s built with `format!`.
- **Model output read from tainted input has no type.** Nothing produces it yet, but readers (S4.2, S4.3) will.

One path today does exactly what this story forbids. `hyalos run --issue-number` puts the issue's title, labels and body into the harness's prompt inside an `<issue>` block ([ADR-0052](0052-reading-the-jobs-issue-from-github.md)). That is safe within today's bounds: the harness runs tainted, and the manifest, sandbox and gates hold whatever it tries. But it is the shape a workflow must not install. The reference workflow (S4.6, #78) will replace it with a reader that turns the issue into typed values. That needs readers in the broker (S4.3), which are not built.

## Decision

**Two kinds of thing can go into a prompt: text and typed values.** Text is free-form and its type says who wrote it. A typed value is drawn from a closed set the schema's author defined, so a model that produced it chose among options, and wrote none of them.

**Text has one type per provenance**, all in `hyalos_types::provenance`:
- `OperatorText`: the operator's request, configuration and policy, and what the engine makes from them, such as the job's branch name and the steps it gives the harness. It is never made from something read from the forge or written by a model.
- `PluginText`: instructions from an installed skill or plugin. Its constructor comes with plugins (S4.8, #88).
- `ForgeText`: as today, forge-authored.
- `ModelText`: free text a model wrote after reading tainted input. Its constructor comes with readers.

`ForgeText` and `ModelText` are **tainted text**. Neither has `as_str`, `Display`, `Deref`, `AsRef<str>` or `Into<String>`. What code outside `hyalos-types` can learn from tainted text is its length, and whether it equals a given text (`PartialEq`, which `ForgeText` already derives). `as_str` is removed. The broker's forge tests compare each field with the `ForgeText` they expect. A digest was considered and left out: `hyalos-types` has no SHA-256, and nothing needs one yet.

**Typed values implement a sealed trait, `Typed`.** The first kinds are `bool`, bounded integers, and `Choice`: one of a closed list of options the schema names, rendered as the option's own text. The option text is the schema author's: the operator's, or a plugin's. A `ReasonCode` is not a kind. Its grammar allows 64 bytes of letters, digits and punctuation, so a model could spell a sentence in one. A code-like value is a `Choice` among codes the schema lists. A typed value comes wrapped in `Sourced<T>`, which carries its provenance, so a `Choice` a reader picked from an issue is still `ModelFromTainted` and still taints the step. The sealed trait means a new kind is a change to `hyalos-types`, reviewed as trusted core. S4.2 adds the kinds readers need. A kind that can carry arbitrary text, such as an unbounded string, a path or a list of strings, is free text by another name and must not be added.

**An actor's prompt is its own type, `ActorPrompt`.** It is built in steps, each taking one typed thing:
- `operator(&OperatorText)`
- `plugin(&PluginText)`
- `value(&Sourced<impl Typed>)`
- `literal(&'static str)` for the engine's own fixed wording.

No method takes `ForgeText` or `ModelText`. Since tainted text has no way out as a `&str`, putting it in an actor's prompt does not compile. `ActorPrompt::inputs()` returns the `StepInputs` from what was actually added, and `mint_manifest` takes those. `hyalos run` therefore plans the prompt before minting. The manifest is minted before the issue can be read, since the read needs the job's token. The provenance comes from the prompt, not from what someone wrote beside it. The finished prompt is a `String` (`into_text`), handed to the substrate as today.

**A reader's prompt is its own type, `ReaderPrompt`.** It takes everything an actor's prompt takes, plus tainted text. A reader has no tools, and the broker refuses any tool on its key ([ADR-0037](0037-model-protocol-adapters-and-their-line-budget.md), S4.3). This is the one place in normal use where tainted text becomes a model's input. It is defined here but used from S4.3 on.

**Only `hyalos-types` reads tainted text through its API.** The readers are `ReaderPrompt` and the direct task below, both in a deterministic, code-owned crate and listed in the module's documentation. Serialization is the other way out. `Serialize` is derived, so any crate can call it, and the engine and broker do, for the channel. It is named under Consequences.

**A run given the operator's text is an ordinary actor prompt.** `hyalos run --issue <text>` builds an `ActorPrompt` from `OperatorText`, so its inputs are `Operator`, and the actor is still tainted by its role.

**The direct run stays, as a named exception, until the reference workflow replaces it.** `hyalos run --issue-number` is not a workflow: it runs one tainted harness actor. Its prompt becomes a `DirectPrompt`, a separate type that `ActorPrompt` does not accept. `ForgeIssue::direct_task()` builds the `<issue>` block, which moves from `run_cmd` into `hyalos-types`, using the same `&lt;` rewriting as before. `DirectPrompt::inputs()` is the inputs of the engine's text before and after the issue, plus `ForgeAuthored`. `DirectPrompt::inputs_with(before, after)` gives the same answer before the issue is read, for minting. The workflow runner (S4.2) takes only `ActorPrompt`, so no workflow can use this path. When S4.6 ships the fix-issue workflow, `--issue-number` runs that workflow instead, and `DirectPrompt` and `direct_task` are deleted. That deletion is part of S4.6's acceptance.

**The tests are compile-fail doctests,** in `hyalos-types`, so they need no new dependency. Rustdoc checks a `compile_fail` test's error code only on a nightly toolchain, and the pinned toolchain is stable: a test given the wrong code still passes. So the error code is not what keeps a test honest. Each compile-fail doctest is paired with a doctest that compiles, and the two differ only in the value passed: tainted in one, untainted in the other. The failure then has to come from that value's type. The code is still written in each test (`E0308` for a mismatched type, `E0277` for a missing trait, `E0599` for a missing method, `E0614` for no `Deref`), to document the failure. They cover:
- `ForgeText` and `ModelText` passed to each `ActorPrompt` method;
- `as_str`, `to_string`, `format!("{}")`, `Deref` and `AsRef<str>` on tainted text;
- a `DirectPrompt` where an `ActorPrompt` is expected.

Unit tests also check that `ActorPrompt::inputs()` reports every provenance it took, and that a `Sourced` value from tainted input taints a reader or planner step.

## Consequences

Taking an issue's text into an actor's prompt by mistake is now a compile error, and the step's provenance is derived from its prompt instead of being asserted beside it. That is the S4.1 acceptance. The install check in S4.2 can build on it: a workflow step's prompt is an `ActorPrompt` or nothing.

**What the type does not stop.** It stops mistakes, not deliberate bypass in trusted code:
- Serializing a `ForgeIssue` with `serde_json` and reading the JSON gives back the text. Serialization has to exist for the channel.
- `OperatorText::new` takes any string, so code that got forge text some other way could relabel it.

Both are visible in review, and all of it is trusted core under code-owner review. The engine never reads the forge itself ([ADR-0052](0052-reading-the-jobs-issue-from-github.md)), so it has no other source of forge text to relabel.

**What it does not cover.** As [ADR-0005](0005-provenance-is-a-type.md) says, the type covers what the engine hands a step, not what a harness reads inside its sandbox. A harness actor still reads its repository and runs tainted. The manifest, sandbox and gates remain the boundary for that.

**The exception is real and has an end.** Until S4.6, the shipped `hyalos run --issue-number` puts forge text before a harness actor, as it does today. Having its own type means it cannot spread, and it is deleted with S4.6. The alternative was to remove `--issue-number` until then. That would take a working, tested path away from operators for no gain in what the sandbox allows.

**Costs.** Building a prompt takes more code than one `format!`. `hyalos-types` gains about 300 lines of trusted core. Typed values with no free-text kind are restrictive by design. A reader that needs to pass on what an issue says, rather than classify it, has to use a gated handle (S4.4, #26), not a string.
