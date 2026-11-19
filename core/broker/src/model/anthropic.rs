//! Anthropic Messages.

use serde_json::{Value, json};

use super::{Adapter, Route};

/// The Anthropic Messages protocol, as Claude Code and the Claude Agent SDK speak it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anthropic;

/// Anthropic-defined tools that the client runs, by exact versioned type. Server tools (web search,
/// web fetch, code execution and its `bash_code_execution_*`, MCP toolsets) and any type not listed
/// here, a newer version of a client tool included, are removed: the harness loses that tool, which
/// is the safe direction.
const CLIENT_TYPES: &[&str] = &[
    "bash_20241022",
    "bash_20250124",
    "text_editor_20241022",
    "text_editor_20250124",
    "text_editor_20250429",
    "text_editor_20250728",
    "computer_20241022",
    "computer_20250124",
    "memory_20250818",
];

impl Adapter for Anthropic {
    fn route(&self, path: &str) -> Option<Route> {
        match path {
            "/v1/messages" => Some(Route::Generate),
            "/v1/messages/count_tokens" => Some(Route::CountTokens),
            _ => None,
        }
    }

    fn credential_header(&self) -> (&'static str, &'static str) {
        ("x-api-key", "")
    }

    fn passthrough(&self) -> &'static [&'static str] {
        &["accept", "anthropic-version", "anthropic-beta"]
    }

    fn defaults(&self) -> &'static [(&'static str, &'static str)] {
        &[("anthropic-version", "2023-06-01")]
    }

    fn harness_runs(&self, tool: &Value) -> bool {
        match tool.get("type") {
            None => true,
            Some(Value::String(t)) => t == "custom" || CLIENT_TYPES.contains(&t.as_str()),
            Some(_) => false,
        }
    }

    fn fields(&self) -> &'static [&'static str] {
        // Not `container` (a hosted execution environment), `mcp_servers` (a hosted connector) or
        // `thread` (a conversation the provider holds).
        &[
            "model",
            "messages",
            "max_tokens",
            "system",
            "metadata",
            "stop_sequences",
            "stream",
            "temperature",
            "top_p",
            "top_k",
            "tools",
            "tool_choice",
            "thinking",
            "service_tier",
            "output_config",
            "output_format",
            "speed",
            "inference_geo",
            "context_management",
            "cache_control",
            "fallbacks",
            "diagnostics",
        ]
    }

    fn output_limit(&self) -> &'static str {
        "max_tokens"
    }

    fn overloaded(&self) -> u16 {
        529
    }

    fn stream_ends(&self) -> &'static [&'static str] {
        &["message_stop"]
    }

    fn error_body(&self, status: u16, message: &str) -> Vec<u8> {
        let kind = match status {
            402 => "billing_error",
            403 => "permission_error",
            404 => "not_found_error",
            413 => "request_too_large",
            529 => "overloaded_error",
            500..=599 => "api_error",
            _ => "invalid_request_error",
        };
        json!({ "type": "error", "error": { "type": kind, "message": message } })
            .to_string()
            .into_bytes()
    }
}
