//! The conformance suite's reference fixture tools, served through the engine's MCP protocol layer
//! (S5.1, ADR-0067).
//!
//! The suite's scenarios call tools by fixed names from its reference server
//! (`examples/servers/typescript/everything-server.ts` in the suite), and a missing one fails rather
//! than skips. These are those tools, with the same names and content, so the scenarios exercise
//! `hyalos::mcp` itself: content blocks of every kind, tool errors, progress over a stream, and the
//! refusal of a call that needs a capability the client did not declare. None of it is in the binary.

use std::net::TcpListener;
use std::sync::Arc;

use hyalos::mcp::{self, Admit, Called, Progress, Refusal, Server, Tool, Tools};
use serde_json::{Map, Value, json};

pub const IMAGE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
pub const AUDIO_WAV: &str = "UklGRiYAAABXQVZFZm10IBAAAAABAAEAQB8AAAB9AAACABAAZGF0YQIAAAA=";

pub struct Fixtures {
    tools: Vec<Tool>,
}

impl Default for Fixtures {
    fn default() -> Self {
        let tool = |name, description, requires| Tool {
            name,
            title: name,
            description,
            input_schema: json!({"type": "object", "additionalProperties": false}),
            output_schema: None,
            requires,
        };
        Self {
            tools: vec![
                tool(
                    "test_simple_text",
                    "Tests simple text content response",
                    &[],
                ),
                tool("test_image_content", "Tests image content response", &[]),
                tool("test_audio_content", "Tests audio content response", &[]),
                tool(
                    "test_embedded_resource",
                    "Tests embedded resource content response",
                    &[],
                ),
                tool(
                    "test_multiple_content_types",
                    "Tests response with multiple content types (text, image, resource)",
                    &[],
                ),
                tool("test_error_handling", "Tests error response handling", &[]),
                tool(
                    "test_tool_with_progress",
                    "Tests tool that reports progress notifications",
                    &[],
                ),
                tool(
                    "test_missing_capability",
                    "Test tool requiring sampling",
                    &["sampling"],
                ),
                tool(
                    "test_logging_tool",
                    "Diagnostic logging validator tool",
                    &[],
                ),
            ],
        }
    }
}

impl Tools for Fixtures {
    fn tools(&self) -> &[Tool] {
        &self.tools
    }

    fn call(&self, name: &str, _: &Map<String, Value>, progress: &mut Progress<'_>) -> Called {
        let blocks = |content: Vec<Value>| Called {
            content,
            structured: None,
            is_error: false,
        };
        let image = json!({"type": "image", "data": IMAGE_PNG, "mimeType": "image/png"});
        match name {
            "test_simple_text" => Called::text("This is a simple text response for testing."),
            "test_image_content" => blocks(vec![image]),
            "test_audio_content" => blocks(vec![
                json!({"type": "audio", "data": AUDIO_WAV, "mimeType": "audio/wav"}),
            ]),
            "test_embedded_resource" => blocks(vec![json!({"type": "resource", "resource": {
                "uri": "test://embedded-resource", "mimeType": "text/plain", "text": "This is an embedded resource content.",
            }})]),
            "test_multiple_content_types" => blocks(vec![
                json!({"type": "text", "text": "Multiple content types test:"}),
                image,
                json!({"type": "resource", "resource": {
                    "uri": "test://mixed-content-resource", "mimeType": "application/json",
                    "text": json!({"test": "data", "value": 123}).to_string(),
                }}),
            ]),
            "test_error_handling" => {
                Called::error("This tool intentionally returns an error for testing")
            }
            "test_tool_with_progress" => {
                for step in [0, 50, 100] {
                    progress.report(step, Some(100), &format!("Completed step {step} of 100"));
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Called::text("Progress test completed")
            }
            // Never reached: the server refuses it first, since no request declares sampling here.
            "test_missing_capability" => Called::text("sampling was declared"),
            "test_logging_tool" => {
                Called::text("Logging tool completed; nothing was logged without a logLevel")
            }
            _ => Called::error("unknown tool"),
        }
    }
}

/// Admits every caller. The suite presents no credential, and these fixtures hold nothing; the
/// engine's server admits only a credential the operator issued (ADR-0068), and lives in `mcp_cmd`.
struct Anyone;

impl Admit for Anyone {
    fn admit(&self, _: Option<&str>) -> Result<(), Refusal> {
        Ok(())
    }
}

/// Serves the fixtures on a free loopback port and returns the endpoint's URL.
pub fn serve_fixtures() -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!(
        "http://localhost:{}{}",
        listener.local_addr()?.port(),
        mcp::http::PATH
    );
    let server = Arc::new(Server::new(
        Fixtures::default(),
        Box::new(Anyone),
        "hyalos-mcp-fixtures",
        "0.0.0",
        "Conformance fixtures.",
    ));
    std::thread::spawn(move || mcp::http::serve(listener, server));
    Ok(url)
}
