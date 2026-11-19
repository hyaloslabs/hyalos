# ADR-0120: The model relay refuses a request field it does not know

**Status:** Accepted. Supersedes [ADR-0117](0117-the-model-relay-passes-only-the-request-fields-it-knows.md)'s rule that an unknown field is dropped.

## Context

[ADR-0117](0117-the-model-relay-passes-only-the-request-fields-it-knows.md) gave each adapter a list of the top-level request fields it passes on, and dropped every other field. Dropping keeps a harness's request running, but it changes what the request means without saying so. Say a harness sends `previous_response_id`. Without the field, the conversation the provider holds is silently lost, and the model answers a different question. Elsewhere the broker refuses what it does not understand (ADR-0009). The operator asked for the same here.

## Decision

The relay refuses a request that has any top-level field outside the adapter's list, before it holds any of the budget or calls the provider. The refusal is 400 `model.field_refused`, in the protocol's error shape. The field is not named in the reply: a field name is text from the sandbox, and nothing the sandbox sent is echoed back to it.

Two fields are on no adapter's list but are known to the relay, which handles them itself whatever the protocol:
- `background` is refused unless it is `false` (ADR-0038).
- `mcp_servers` is removed as a hosted connector, and refused for a reader.

The adapters' lists are ADR-0117's, unchanged.

## Consequences

- A request means what the provider receives, or it is refused. A harness learns at once that a field is not served, instead of getting a quietly different answer.
- A harness that sends a field not on the list stops working until the field is added to the adapter's list. Claude Code and Codex send fields on the lists today. A field a provider adds later, once a harness sends it, refuses every call. That change to the trusted core is the review ADR-0117 asked for, now made necessary.
