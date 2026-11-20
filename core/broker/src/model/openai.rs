//! OpenAI Responses, which also covers local OpenAI-compatible servers.

use serde_json::{Map, Value, json};

use super::Adapter;

/// Tool types the client runs. Hosted tools (web search, file search, code interpreter, hosted MCP,
/// image generation) and any type not listed here are removed.
const CLIENT_TYPES: &[&str] = &[
    "function",
    "custom",
    "local_shell",
    "apply_patch",
    "computer_use_preview",
];

/// `include` values that ask for a hosted tool's output.
const HOSTED_INCLUDES: &[&str] = &[
    "web_search_call",
    "file_search_call",
    "code_interpreter_call",
    "mcp_",
];

/// The OpenAI Responses protocol, as Codex and the OpenAI Agents SDK speak it.
pub static OPENAI: Adapter = Adapter {
    generate: "/v1/responses",
    count_tokens: "/v1/responses/input_tokens",
    credential_header: ("authorization", "Bearer "),
    passthrough: &["accept"],
    defaults: &[],
    harness_runs,
    scrub,
    // Not `prompt` (a template the provider stores, with its own tools), `previous_response_id` or
    // `conversation` (state the provider holds), nor `background`, refused before this.
    fields: &[
        "model",
        "input",
        "instructions",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "parallel_tool_calls",
        "reasoning",
        "store",
        "stream",
        "stream_options",
        "temperature",
        "top_p",
        "top_logprobs",
        "text",
        "tools",
        "tool_choice",
        "truncation",
        "include",
        "service_tier",
        "prompt_cache_key",
        "safety_identifier",
        "user",
    ],
    output_limit: "max_output_tokens",
    overloaded: 503,
    stream_ends: &[
        "response.completed",
        "response.incomplete",
        "response.failed",
    ],
    error_body,
    quota_exhausted,
    quota_event,
};

fn harness_runs(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| CLIENT_TYPES.contains(&t))
}

fn scrub(body: &mut Map<String, Value>) {
    if let Some(Value::Array(include)) = body.get_mut("include") {
        include.retain(|v| {
            !v.as_str()
                .is_some_and(|s| HOSTED_INCLUDES.iter().any(|p| s.starts_with(p)))
        });
        if include.is_empty() {
            body.remove("include");
        }
    }
}

fn error_body(status: u16, message: &str) -> Vec<u8> {
    let kind = match status {
        402 => "insufficient_quota",
        500..=599 => "server_error",
        _ => "invalid_request_error",
    };
    json!({ "error": { "message": message, "type": kind, "param": null, "code": kind } })
        .to_string()
        .into_bytes()
}

fn quota_exhausted(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    ["code", "type"]
        .iter()
        .any(|k| value.pointer(&format!("/error/{k}")) == Some(&json!("insufficient_quota")))
}

/// An `error` event, or a `response.failed` one, whose code is `insufficient_quota`.
fn quota_event(event: &Value) -> bool {
    [
        "/code",
        "/error/code",
        "/error/type",
        "/response/error/code",
    ]
    .iter()
    .any(|p| event.pointer(p) == Some(&json!("insufficient_quota")))
}
