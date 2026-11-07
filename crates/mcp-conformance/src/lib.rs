//! The conformance suite's reference fixture tools, served through the engine's MCP protocol layer
//! (S5.1, ADR-0067).
//!
//! The suite's scenarios call tools by fixed names from its reference server
//! (`examples/servers/typescript/everything-server.ts` in the suite), and a missing one fails rather
//! than skips. These are those tools, with the same names and content, so the scenarios exercise
//! `hyalos::mcp` itself: content blocks of every kind, tool errors, progress over a stream, and the
//! refusal of a call that needs a capability the client did not declare. None of it is in the binary.
//!
//! The Tasks extension's scenarios (SEP-2663, ADR-0069) call their own fixtures: `greet`,
//! `slow_compute`, `failing_job`, `protocol_error_job` and `confirm_delete`. Their tasks live in
//! memory here, and each one's state follows from the time since it started.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use hyalos::mcp::{
    self, Admit, Called, Progress, Refusal, Server, TaskState, TaskSupport, TaskView, Tool, Tools,
};
use serde_json::{Map, Value, json};

pub const IMAGE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
pub const AUDIO_WAV: &str = "UklGRiYAAABXQVZFZm10IBAAAAABAAEAQB8AAAB9AAACABAAZGF0YQIAAAA=";

pub struct Fixtures {
    tools: Vec<Tool>,
    tasks: Mutex<Tasks>,
}

/// The fixtures' tasks, by id.
#[derive(Default)]
struct Tasks {
    next: u64,
    started: BTreeMap<String, Started>,
}

#[derive(Clone)]
struct Started {
    kind: Kind,
    at: Instant,
    created_ms: u64,
    /// How long it runs before it ends.
    takes: Duration,
    cancelled: Option<Instant>,
}

#[derive(Clone)]
enum Kind {
    Compute(String),
    Failing,
    ProtocolError,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn view(id: &str, task: &Started) -> TaskView {
    let elapsed = task.at.elapsed();
    let since = |instant: Instant| instant.duration_since(task.at).as_millis() as u64;
    let ended = elapsed >= task.takes;
    let (state, updated) = match (&task.cancelled, ended) {
        // A cancel after it ended changes nothing.
        (Some(at), _) if *at < task.at + task.takes => (TaskState::Cancelled, since(*at)),
        (_, false) => (TaskState::Working, 0),
        (_, true) => (
            match &task.kind {
                Kind::Compute(label) => {
                    TaskState::Completed(Called::text(&format!("Computed {label}")))
                }
                Kind::Failing => {
                    TaskState::Completed(Called::error("This job intentionally fails"))
                }
                Kind::ProtocolError => TaskState::Failed {
                    code: -32603,
                    message: "This job intentionally ends in a protocol error".into(),
                },
            },
            task.takes.as_millis() as u64,
        ),
    };
    TaskView {
        id: id.to_string(),
        created_ms: task.created_ms,
        updated_ms: task.created_ms + updated,
        ttl_ms: None,
        poll_ms: Some(200),
        message: None,
        state,
    }
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
            task: TaskSupport::Never,
        };
        let with = |name, description, properties: Value, task| Tool {
            input_schema: json!({"type": "object", "properties": properties}),
            task,
            ..tool(name, description, &[])
        };
        Self {
            tasks: Mutex::default(),
            tools: vec![
                with(
                    "greet",
                    "Greets by name",
                    json!({"name": {"type": "string"}}),
                    TaskSupport::Never,
                ),
                with(
                    "slow_compute",
                    "Sleeps for seconds, then answers; a task when the client has the Tasks extension",
                    json!({"seconds": {"type": "number"}, "label": {"type": "string"}}),
                    TaskSupport::Optional,
                ),
                with(
                    "failing_job",
                    "A task that ends with a tool error",
                    json!({}),
                    TaskSupport::Required,
                ),
                with(
                    "protocol_error_job",
                    "A task that ends in a protocol error",
                    json!({}),
                    TaskSupport::Required,
                ),
                // The suite's own parks for input, which this server never asks for: a plain call.
                with(
                    "confirm_delete",
                    "Answers at once; asks for no input",
                    json!({"filename": {"type": "string"}}),
                    TaskSupport::Never,
                ),
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

    fn call(
        &self,
        name: &str,
        arguments: &Map<String, Value>,
        progress: &mut Progress<'_>,
    ) -> Called {
        let text = |key: &str| arguments.get(key).and_then(Value::as_str).unwrap_or("");
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
            "greet" => Called::text(&format!("Hello, {}!", text("name"))),
            "slow_compute" => {
                std::thread::sleep(seconds(arguments));
                Called::text(&format!("Computed {}", text("label")))
            }
            "confirm_delete" => Called::text(&format!("Nothing deleted: {}", text("filename"))),
            _ => Called::error("unknown tool"),
        }
    }

    fn start(&self, name: &str, arguments: &Map<String, Value>) -> Result<TaskView, Called> {
        let (kind, takes) = match name {
            "slow_compute" => {
                let label = arguments.get("label").and_then(Value::as_str);
                (
                    Kind::Compute(label.unwrap_or("").into()),
                    seconds(arguments),
                )
            }
            "failing_job" => (Kind::Failing, Duration::from_secs(1)),
            "protocol_error_job" => (Kind::ProtocolError, Duration::from_millis(500)),
            _ => return Err(Called::error("this tool does not run as a task")),
        };
        let mut tasks = self.tasks.lock().map_err(|_| Called::error("poisoned"))?;
        tasks.next += 1;
        let id = format!("fixture-task-{}", tasks.next);
        let started = Started {
            kind,
            at: Instant::now(),
            created_ms: now_ms(),
            takes,
            cancelled: None,
        };
        let shown = view(&id, &started);
        tasks.started.insert(id, started);
        Ok(shown)
    }

    fn task(&self, id: &str) -> Option<TaskView> {
        let tasks = self.tasks.lock().ok()?;
        tasks.started.get(id).map(|t| view(id, t))
    }

    fn cancel(&self, id: &str) -> bool {
        let Ok(mut tasks) = self.tasks.lock() else {
            return false;
        };
        match tasks.started.get_mut(id) {
            Some(task) => {
                task.cancelled.get_or_insert_with(Instant::now);
                true
            }
            None => false,
        }
    }
}

/// `seconds`, at most a minute.
fn seconds(arguments: &Map<String, Value>) -> Duration {
    let n = arguments
        .get("seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Duration::from_secs_f64(n.clamp(0.0, 60.0))
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
