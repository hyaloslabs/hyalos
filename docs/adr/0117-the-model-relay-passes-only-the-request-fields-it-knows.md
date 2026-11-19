# ADR-0117: The model relay passes only the request fields it knows

**Status:** Proposed

## Context

The model relay (ADR-0037, ADR-0038) passes a job's request to the provider with the key added. It removes provider-hosted tools, `mcp_servers` and a background response, and refuses tools from a reader. Every other top-level field reached the provider unchanged.

Some of those fields name work or state the broker never sees. OpenAI's `prompt` runs a template stored with the provider, which can carry its own instructions and tools. `previous_response_id` and `conversation` continue a conversation the provider holds. Anthropic's `container` names a hosted execution environment, and `thread` a conversation the provider keeps. The reader check (`declares_tools`) looks only at the request's own `tools` and `mcp_servers`, so a stored prompt's tools passed it. A field added to a provider's API later would pass the same way, and `anthropic-beta` turns on beta fields. The pre-read of the trusted core found this (#270, #339).

## Decision

Each adapter names the top-level request fields it passes on. The relay drops every other field, as it drops every request header not on the adapter's list. The fields are applied after the background and reader checks and before hosted tools are removed, on both the generation and the counting path.

- **Anthropic.** It passes `model`, `messages`, `max_tokens`, `system`, `metadata`, `stop_sequences`, `stream`, `temperature`, `top_p`, `top_k`, `tools`, `tool_choice`, `thinking`, `service_tier`, `output_config`, `output_format`, `speed`, `inference_geo`, `context_management`, `cache_control`, `fallbacks` and `diagnostics`. It drops `container`, `mcp_servers` and `thread`.
- **OpenAI Responses.** It passes `model`, `input`, `instructions`, `max_output_tokens`, `max_tool_calls`, `metadata`, `parallel_tool_calls`, `reasoning`, `store`, `stream`, `stream_options`, `temperature`, `top_p`, `top_logprobs`, `text`, `tools`, `tool_choice`, `truncation`, `include`, `service_tier`, `prompt_cache_key`, `safety_identifier` and `user`. It drops `prompt`, `previous_response_id` and `conversation`.

A dropped field is dropped, not refused. That is the same rule the relay uses for headers: an unknown field fails closed, and the harness's request still runs.

## Consequences

- A request can steer the provider only through fields the broker knows, and the reader check sees every tool the provider will see.
- A harness that relies on provider-held state must send its whole conversation in each request. Claude Code and Codex already do.
- A field the providers add later does nothing until it is added to the adapter's list. That needs a change to the trusted core and a review of what the field does.
