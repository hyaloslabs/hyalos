//! OpenAI Responses, which also covers local OpenAI-compatible servers.

use serde_json::{Map, Value, json};

use super::{Adapter, Route};

/// The OpenAI Responses protocol, as Codex and the OpenAI Agents SDK speak it.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAi;

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

impl Adapter for OpenAi {
    fn route(&self, path: &str) -> Option<Route> {
        match path {
            "/v1/responses" => Some(Route::Generate),
            "/v1/responses/input_tokens" => Some(Route::CountTokens),
            _ => None,
        }
    }

    fn credential_header(&self) -> (&'static str, &'static str) {
        ("authorization", "Bearer ")
    }

    fn passthrough(&self) -> &'static [&'static str] {
        &["accept"]
    }

    fn harness_runs(&self, tool: &Value) -> bool {
        tool.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| CLIENT_TYPES.contains(&t))
    }

    fn scrub(&self, body: &mut Map<String, Value>) {
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

    fn output_limit(&self) -> &'static str {
        "max_output_tokens"
    }

    fn overloaded(&self) -> u16 {
        503
    }

    fn stream_ends(&self) -> &'static [&'static str] {
        &[
            "response.completed",
            "response.incomplete",
            "response.failed",
        ]
    }

    fn error_body(&self, status: u16, message: &str) -> Vec<u8> {
        let kind = match status {
            402 => "insufficient_quota",
            500..=599 => "server_error",
            _ => "invalid_request_error",
        };
        json!({ "error": { "message": message, "type": kind, "param": null, "code": kind } })
            .to_string()
            .into_bytes()
    }

    fn quota_exhausted(&self, body: &[u8]) -> bool {
        let Ok(value) = serde_json::from_slice::<Value>(body) else {
            return false;
        };
        ["code", "type"]
            .iter()
            .any(|k| value.pointer(&format!("/error/{k}")) == Some(&json!("insufficient_quota")))
    }
}
