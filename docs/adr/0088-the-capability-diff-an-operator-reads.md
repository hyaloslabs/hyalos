# ADR-0088: The capability diff an operator reads

**Status:** Proposed

## Context

[ADR-0006](0006-plugins-are-skills-and-wasm-components.md) and [ADR-0012](0012-humans-approve-only-irreversible-actions.md) make every capability grant a human's decision. A local install starts with none, and an update that adds one waits for consent ([ADR-0007](0007-capability-based-marketplace-trust.md)). ADR-0006 says this costs human attention, so the capability diff has to be easy to read.

[ADR-0085](0085-the-capability-vocabulary.md) defined `Capabilities::diff`, which returns what a newer set adds and what it takes away. S6.4 (#39) asks for the diff an operator reads before any install or update, listing added, removed and unchanged capabilities. Install (S6.2, S6.7, S7.9) and update (S7.10) come after this story, so nothing yet shows the diff.

## Decision

**A diff has three parts.** `Diff` holds `added`, `removed` and `unchanged`, each a `Capabilities`. Every name in either set is in exactly one of the three. `widens` is still true exactly when `added` is not empty, and that is still what needs consent. A first install is the diff from `Capabilities::none()`, so everything it asks for is added.

**One rendering, in the `hyalos` crate (`plugin_cmd::render`).** It prints:
- a count line: `capabilities: N added, N removed, N unchanged`;
- each of the four fields in vocabulary order, with each name marked `+` (added), `-` (removed) or a space (unchanged), sorted by name, and `(none)` for an empty field;
- a last line that says whether the change adds capabilities, and so needs a human to grant each one.

Install and update print this same text before they ask for consent, so an operator always reads one format.

**Plugin-authored names are printed, and that is safe.** The names come from a plugin's document, which is plugin-authored text. They reach only the operator's terminal, never the sandbox, the agent or a log line. Each one has passed the vocabulary's check (`Capabilities::from_document`) before it is printed. So it is an action or flow from a closed list, a host name, or a tool name of ASCII letters, digits, `_`, `-` and `.`, and cannot hold a control character or an escape sequence. A document that fails the check is refused with the field's name and never echoed.

**`hyalos plugin diff [<installed>] <new>`** reads one or two capabilities documents, writes nothing and prints the rendering. With one file, it shows a first install. Exit 0 means the documents were read, 1 that one could not be read or was refused, and 2 is usage. It is an operator tool with no MCP tool, like `workflow check` ([ADR-0075](0075-the-thin-cli-for-the-agents-tools.md)): it inspects files on the operator's machine and decides nothing.

**A capabilities document is at most 64 KiB** (`MAX_CAPABILITY_DOCUMENT`). `from_document` refuses a larger one as malformed before parsing it, so every reader is bounded, not only this command.

## Consequences

An operator reads the full picture, including what an update keeps, not only what it changes. Unchanged lines are context: the consent decision rests on `added` alone.

The rendering is plain text and its format is pinned by a test. Install and update reuse it rather than inventing their own. A machine-readable form (`--json`) can be added when a caller needs one.

Tests:
- `capability.rs` (unit): every name of either set is in exactly one part; a first install adds everything; a document over the size limit is refused.
- `plugin_cmd.rs` (unit): an update's rendering is pinned; a narrowing update adds nothing; a first install adds everything; a refused document is not echoed; usage.
