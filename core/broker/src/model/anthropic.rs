//! Anthropic Messages.

use serde_json::{Value, json};

use super::{Adapter, Route};

/// The Anthropic Messages protocol, as Claude Code and the Claude Agent SDK speak it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anthropic;

/// Anthropic-defined tools that the client runs. Server tools (web search, web fetch, code
/// execution, MCP toolsets) and any type not listed here are removed.
const CLIENT_TYPES: &[&str] = &["bash_", "text_editor_", "computer_", "memory_"];

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
            Some(Value::String(t)) => {
                t == "custom" || CLIENT_TYPES.iter().any(|prefix| t.starts_with(prefix))
            }
            Some(_) => false,
        }
    }

    fn overloaded(&self) -> u16 {
        529
    }

    fn error_body(&self, status: u16, message: &str) -> Vec<u8> {
        let kind = match status {
            402 => "billing_error",
            403 => "permission_error",
            404 => "not_found_error",
            413 => "request_too_large",
            529 => "overloaded_error",
            _ => "invalid_request_error",
        };
        json!({ "type": "error", "error": { "type": kind, "message": message } })
            .to_string()
            .into_bytes()
    }
}
