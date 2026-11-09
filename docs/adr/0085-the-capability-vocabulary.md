# ADR-0085: The capability vocabulary

**Status:** Proposed

## Context

[ADR-0004](0004-one-signed-manifest-per-job.md) says the job manifest and plugin capabilities share one versioned vocabulary, and that a field that widens authority needs an ADR. [ADR-0031](0031-the-signed-manifest-and-its-admission.md) left the vocabulary of action and tool names to the story that defines it. [ADR-0066](0066-plugin-authored-text-and-the-grant-that-bounds-it.md) wrote a plugin's `Grant` in the manifest's own sets until that story (S6.6, #82). [ADR-0006](0006-plugins-are-skills-and-wasm-components.md) says the capability surface must stay small, and [ADR-0007](0007-capability-based-marketplace-trust.md) rests a plugin's security on it. A plugin's update publishes or installs on its own when it asks for nothing new, so the diff between two sets of capabilities decides what needs a human.

Today the four capability sets are plain names:
- The mint refuses an action the policy does not know, and a data flow this build does not know.
- Any string passes as a tool or an egress host.
- Nothing defines a plugin's capabilities as a document, a version, or a diff.

## Decision

**One vocabulary, in `hyalos_types::capability`.** It has four fields. Each is asked of the policy as one action, and each accepts a closed set of names or names of one syntax:

| Field | Asked as | Accepts |
| --- | --- | --- |
| `actions` | the action's own name | `read`, `push`, `open_pr`, `comment`, `merge`, `change_policy`, `publish`, `spend_over_cap`, `export_credentials` |
| `egress_hosts` | `request_host` | a host name in the form the broker's egress gate compares (`egress::host_name`): lower case, dot-separated labels of 1 to 63 letters, digits and inner hyphens, at most 253 bytes, no trailing dot. No port, no wildcard, no scheme, and no address, including a numeric last label or `0x...` |
| `tools` | `use_tool` | 1 to 128 ASCII letters, digits, `_`, `-` and `.`, the names MCP allows |
| `data_flows` | `data_flow` | `job->model`, `job->forge`, `job->web` ([ADR-0055](0055-data-flows-name-where-the-jobs-data-may-go.md)) |

A test in `hyalos-policy` checks that the actions, together with what the other fields are asked as, are exactly the policy schema's actions. So nothing a manifest or a plugin holds goes unasked, and the policy has no action the vocabulary cannot name.

**`Capabilities` is the type.** It holds the four sets, with no other field (`deny_unknown_fields`). Its operations:
- `check` refuses a name its field does not accept.
- `within` and `allows` test one set of capabilities, or a manifest's, against another.
- `bound` cuts a manifest to the intersection, as `Grant::bound` did.
- `diff` returns what a newer set adds and what it takes away, field by field. `Diff::widens` is true when anything was added: that is what needs consent ([ADR-0007](0007-capability-based-marketplace-trust.md)) and what S6.4 shows.

`Grant` becomes a name for `Capabilities`, so ADR-0066's grant, bound and check are unchanged in meaning and in what they accept from serde.

**The manifest holds the same fields.** `Manifest::capabilities()` returns its four sets as `Capabilities`. The manifest's fields and signed bytes do not change, so there is no manifest version bump ([ADR-0031](0031-the-signed-manifest-and-its-admission.md)). A test checks that the manifest writes every field of the vocabulary under the same name.

**The mint checks the vocabulary.** Besides its existing refusals of an unknown action (`mint.unknown_action`) and an unknown flow (`mint.unknown_flow`), the engine refuses to sign a manifest with:
- a tool its field does not accept (`mint.unknown_tool`);
- an egress host its field does not accept (`mint.unknown_host`).

A test in `hyalos-broker` checks that the vocabulary accepts a host exactly when `host_name` would leave it unchanged. So every host a manifest holds is one the gate can match, as written.

**A plugin's capabilities are a versioned document.** It is written as `{"version": 1, "actions": [...], "egress_hosts": [...], "tools": [...], "data_flows": [...]}`, with absent fields empty. `Capabilities::from_document` reads it, and refuses three things rather than skip them:
- a version other than `CAPABILITY_VERSION` (1);
- an unknown field;
- a name its field does not accept.

The error names the field, never the refused name, since a plugin's document is plugin-authored text. Deserializing `Capabilities` directly checks no name, so anything read from outside, such as a plugin's capabilities at install, goes through `from_document`.

**Changing the vocabulary needs an ADR.** That means a new field, a field removed or renamed, or a field accepting other names. It bumps `CAPABILITY_VERSION`. `core/types/tests/capability.rs` pins the version, the fields in order, what each is asked as, the actions, the flows and the written form, and its failure message says so. Adding a policy action without a field or vocabulary name for it fails the policy test.

## Consequences

The manifest, a plugin's request, a human's grant and the diff an operator reads are now one type, so S6.1's host imports, S6.2's install and S6.4's diff build on it rather than on four loose sets. A plugin can never be granted something the policy cannot be asked about.

The vocabulary is small on purpose: four fields, nine actions and three flows. A plugin that needs something else needs an ADR first, and older builds refuse its document by version rather than ignoring the new field.

Tool names are syntax-checked but not enumerated. Which tools exist depends on the harness and on the MCP servers a job runs ([ADR-0006](0006-plugins-are-skills-and-wasm-components.md)). So a tool is granted by name, and the policy decides each `use_tool` ask.

A manifest host the gate would have rewritten or refused is now refused at the mint with `mint.unknown_host`:
- `docs.rs:443`, `*.example`, a scheme or an address never matched a request, though it was signed;
- `Docs.rs` or `docs.rs.` did match, after the gate lower-cased it or dropped the dot.

`hyalos run` and `start_job` check `--allow-host` against the vocabulary before the task starts, so such a host is refused before any step spends model budget, and the mint's refusal stays as the backstop. An operator who gave `--allow-host` in upper case now writes it in lower case. Refusing it is the fail-closed choice: the manifest says exactly the names that are matched.

Tests:
- `capability.rs` (unit): each field accepts only its own names; a document is read only whole and in this version, and its error names no refused name; the diff says what an update adds and takes away.
- `core/types/tests/capability.rs`: the vocabulary is pinned; the manifest holds every field.
- `hyalos-policy`: the vocabulary is asked as exactly the policy's actions.
- `hyalos-broker` (`egress.rs`): the vocabulary's hosts are the gate's own form.
- `policy_store.rs`: the mint refuses a malformed tool or host.
- `run_cmd` (unit): `hyalos run` refuses a malformed `--allow-host`.
