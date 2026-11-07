//! The MCP server (ADR-0010, ADR-0067): revision 2026-07-28, stateless, tools only.
//!
//! [`Server::handle`] answers one JSON-RPC message by itself, with nothing kept between calls. The
//! transports are [`stdio`] (newline-delimited, what a client that starts `hyalos mcp` speaks) and
//! [`http`] (Streamable HTTP on loopback only). What the server offers is a [`Tools`]; the engine's are
//! in `mcp_cmd`, and the conformance suite's fixtures are in `crates/mcp-conformance`, never here.
//!
//! The order of checks is the revision's: a message that is not JSON-RPC, then `_meta` (`-32602`),
//! then on HTTP the headers against the body (`-32020`), then the version (`-32022`), then the method
//! (`-32601`), then the tool and the capabilities it needs (`-32602`, `-32021`).

pub mod http;
pub mod stdio;

use serde_json::{Map, Value, json};

/// The one revision this server speaks.
pub const PROTOCOL_VERSION: &str = "2026-07-28";
/// The largest message read, on either transport.
pub const MAX_MESSAGE: usize = 1 << 20;
/// How long a client may keep `server/discover` and `tools/list`. The tools never change while the
/// server runs.
pub const TTL_MS: u64 = 300_000;

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// The JSON-RPC error codes this server sends. Nothing else in the reserved range is ever sent.
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const HEADER_MISMATCH: i64 = -32020;
    pub const MISSING_CAPABILITY: i64 = -32021;
    pub const UNSUPPORTED_VERSION: i64 = -32022;
}

/// One tool, as `tools/list` shows it.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// A JSON Schema object.
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    /// Client capabilities a call needs. A call whose request did not declare one is refused with
    /// `-32021` before the tool runs.
    pub requires: &'static [&'static str],
}

/// What a tool returns: its content blocks, structured content if it has an output schema, and
/// whether it failed. A failure the caller can act on is a result with `is_error`, not a protocol
/// error.
#[derive(Debug, Clone, PartialEq)]
pub struct Called {
    pub content: Vec<Value>,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl Called {
    pub fn text(text: &str) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": text})],
            structured: None,
            is_error: false,
        }
    }

    pub fn error(text: &str) -> Self {
        Self {
            is_error: true,
            ..Self::text(text)
        }
    }

    /// `value` as structured content, and as JSON text for a client that reads only text.
    pub fn structured(value: Value) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": value.to_string()})],
            structured: Some(value),
            is_error: false,
        }
    }
}

/// What a server offers.
pub trait Tools: Send + Sync {
    /// Every tool, always in the same order.
    fn tools(&self) -> &[Tool];
    /// Runs one listed tool. `arguments` is the call's object (empty if it gave none).
    fn call(
        &self,
        name: &str,
        arguments: &Map<String, Value>,
        progress: &mut Progress<'_>,
    ) -> Called;
}

/// Reports a call's progress, if its request asked with a `progressToken`; otherwise it does nothing.
pub struct Progress<'a> {
    token: Option<Value>,
    emit: &'a mut dyn FnMut(Value),
}

impl Progress<'_> {
    pub fn report(&mut self, progress: u64, total: Option<u64>, message: &str) {
        let Some(token) = &self.token else { return };
        let mut params = json!({"progressToken": token, "progress": progress, "message": message});
        if let (Some(total), Some(map)) = (total, params.as_object_mut()) {
            map.insert("total".into(), total.into());
        }
        (self.emit)(
            json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": params}),
        );
    }
}

/// The MCP headers of an HTTP request, decoded (ADR-0067). Absent on stdio.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    pub protocol_version: Option<String>,
    pub method: Option<String>,
    pub name: Option<String>,
}

/// The answer to one message: the HTTP status the revision gives it, and the JSON-RPC response, or
/// none for a notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Option<Value>,
}

#[derive(Debug)]
struct Failure {
    code: i64,
    message: &'static str,
    data: Option<Value>,
}

const fn fail(code: i64, message: &'static str) -> Failure {
    Failure {
        code,
        message,
        data: None,
    }
}

fn supported() -> Value {
    json!({"supported": [PROTOCOL_VERSION]})
}

pub struct Server<T> {
    tools: T,
    name: &'static str,
    version: &'static str,
    instructions: &'static str,
}

impl<T: Tools> Server<T> {
    pub fn new(
        tools: T,
        name: &'static str,
        version: &'static str,
        instructions: &'static str,
    ) -> Self {
        Self {
            tools,
            name,
            version,
            instructions,
        }
    }

    /// Answers one message. `headers` are the request's MCP headers on HTTP and `None` on stdio.
    /// Notifications the call sends before its result, such as progress, go to `emit`.
    pub fn handle(
        &self,
        message: &[u8],
        headers: Option<&Headers>,
        emit: &mut dyn FnMut(Value),
    ) -> Reply {
        let Ok(Value::Object(mut message)) = serde_json::from_slice::<Value>(message) else {
            let code = match serde_json::from_slice::<Value>(message) {
                Ok(_) => codes::INVALID_REQUEST,
                Err(_) => codes::PARSE_ERROR,
            };
            return error(None, &fail(code, "Not a JSON-RPC request object"));
        };
        let id = match message.get("id") {
            None => None,
            Some(id @ (Value::String(_) | Value::Number(_))) => Some(id.clone()),
            Some(_) => {
                return error(
                    None,
                    &fail(
                        codes::INVALID_REQUEST,
                        "The id must be a string or a number",
                    ),
                );
            }
        };
        let method = match message.get("method") {
            Some(Value::String(method)) if message.get("jsonrpc") == Some(&json!("2.0")) => {
                method.clone()
            }
            _ => {
                return error(
                    id,
                    &fail(codes::INVALID_REQUEST, "Not a JSON-RPC 2.0 request"),
                );
            }
        };
        // A notification gets no answer; the server acts on none (stdio's `notifications/cancelled`
        // has nothing to cancel, since a call is answered before the next message is read).
        let Some(id) = id else {
            return Reply {
                status: 202,
                body: None,
            };
        };
        let params = match message.remove("params") {
            None => Map::new(),
            Some(Value::Object(params)) => params,
            Some(_) => {
                return error(
                    Some(id),
                    &fail(codes::INVALID_PARAMS, "params must be an object"),
                );
            }
        };
        match self.request(&method, &params, headers, emit) {
            Ok(result) => Reply {
                status: 200,
                body: Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
            },
            Err(failure) => error(Some(id), &failure),
        }
    }

    fn request(
        &self,
        method: &str,
        params: &Map<String, Value>,
        headers: Option<&Headers>,
        emit: &mut dyn FnMut(Value),
    ) -> Result<Value, Failure> {
        // A client of an older revision starts with `initialize` and no `_meta`: tell it which
        // revision this is rather than that its request is malformed.
        if method == "initialize" {
            return Err(Failure {
                data: Some(supported()),
                ..fail(
                    codes::METHOD_NOT_FOUND,
                    "Method not found: this server speaks the stateless revision only",
                )
            });
        }
        let meta = params.get("_meta").and_then(Value::as_object);
        let version = meta
            .and_then(|m| m.get(META_VERSION))
            .and_then(Value::as_str);
        let capabilities = meta
            .and_then(|m| m.get(META_CAPABILITIES))
            .and_then(Value::as_object);
        let (Some(version), Some(capabilities)) = (version, capabilities) else {
            return Err(fail(
                codes::INVALID_PARAMS,
                "_meta must give io.modelcontextprotocol/protocolVersion and io.modelcontextprotocol/clientCapabilities",
            ));
        };
        if let Some(headers) = headers {
            check_headers(headers, method, version, params)?;
        }
        if version != PROTOCOL_VERSION {
            let mut data = supported();
            if let Some(map) = data.as_object_mut() {
                map.insert("requested".into(), version.into());
            }
            return Err(Failure {
                data: Some(data),
                ..fail(codes::UNSUPPORTED_VERSION, "Unsupported protocol version")
            });
        }
        let mut result = match method {
            "server/discover" => self.discover(),
            "tools/list" => self.list(params)?,
            "tools/call" => self.call(params, capabilities, emit)?,
            _ => return Err(fail(codes::METHOD_NOT_FOUND, "Method not found")),
        };
        result.insert("resultType".into(), "complete".into());
        result.insert(
            "_meta".into(),
            json!({(META_SERVER_INFO): {"name": self.name, "version": self.version}}),
        );
        Ok(Value::Object(result))
    }

    fn discover(&self) -> Map<String, Value> {
        object(json!({
            "supportedVersions": [PROTOCOL_VERSION],
            "capabilities": {"tools": {}},
            "instructions": self.instructions,
            "ttlMs": TTL_MS,
            "cacheScope": "public",
        }))
    }

    fn list(&self, params: &Map<String, Value>) -> Result<Map<String, Value>, Failure> {
        // Every tool fits on one page, so no cursor was ever given out.
        if params.contains_key("cursor") {
            return Err(fail(codes::INVALID_PARAMS, "Unknown cursor"));
        }
        let tools: Vec<Value> = self.tools.tools().iter().map(describe).collect();
        Ok(object(
            json!({"tools": tools, "ttlMs": TTL_MS, "cacheScope": "public"}),
        ))
    }

    fn call(
        &self,
        params: &Map<String, Value>,
        capabilities: &Map<String, Value>,
        emit: &mut dyn FnMut(Value),
    ) -> Result<Map<String, Value>, Failure> {
        let name = params.get("name").and_then(Value::as_str);
        let Some(tool) = self.tools.tools().iter().find(|t| Some(t.name) == name) else {
            return Err(fail(codes::INVALID_PARAMS, "Unknown tool"));
        };
        let empty = Map::new();
        let arguments = match params.get("arguments") {
            None => &empty,
            Some(Value::Object(arguments)) => arguments,
            Some(_) => return Err(fail(codes::INVALID_PARAMS, "arguments must be an object")),
        };
        let missing: Map<String, Value> = tool
            .requires
            .iter()
            .filter(|c| !capabilities.contains_key(**c))
            .map(|c| ((*c).to_string(), json!({})))
            .collect();
        if !missing.is_empty() {
            return Err(Failure {
                data: Some(json!({"requiredCapabilities": missing})),
                ..fail(
                    codes::MISSING_CAPABILITY,
                    "Missing required client capability",
                )
            });
        }
        let token = params
            .get("_meta")
            .and_then(|m| m.get("progressToken"))
            .filter(|t| t.is_string() || t.is_number())
            .cloned();
        let called = self
            .tools
            .call(tool.name, arguments, &mut Progress { token, emit });
        let mut result = object(json!({"content": called.content, "isError": called.is_error}));
        if let Some(structured) = called.structured {
            result.insert("structuredContent".into(), structured);
        }
        Ok(result)
    }
}

/// The revision's header rules (ADR-0067): `MCP-Protocol-Version` and `Mcp-Method` always, and
/// `Mcp-Name` on a call, each equal to the body. The message never repeats a header's value.
fn check_headers(
    headers: &Headers,
    method: &str,
    version: &str,
    params: &Map<String, Value>,
) -> Result<(), Failure> {
    if headers.protocol_version.as_deref() != Some(version) {
        return Err(fail(
            codes::HEADER_MISMATCH,
            "Header mismatch: MCP-Protocol-Version",
        ));
    }
    if headers.method.as_deref() != Some(method) {
        return Err(fail(codes::HEADER_MISMATCH, "Header mismatch: Mcp-Method"));
    }
    if matches!(method, "tools/call" | "resources/read" | "prompts/get") {
        let named = params
            .get("name")
            .or_else(|| params.get("uri"))
            .and_then(Value::as_str);
        if named.is_none() || headers.name.as_deref() != named {
            return Err(fail(codes::HEADER_MISMATCH, "Header mismatch: Mcp-Name"));
        }
    }
    Ok(())
}

fn describe(tool: &Tool) -> Value {
    let mut out = json!({
        "name": tool.name,
        "title": tool.title,
        "description": tool.description,
        "inputSchema": tool.input_schema,
    });
    if let (Some(schema), Some(map)) = (&tool.output_schema, out.as_object_mut()) {
        map.insert("outputSchema".into(), schema.clone());
    }
    out
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// An error response. Its id is left out only when the request's could not be read. The HTTP status
/// is the revision's: 404 for an unknown method, 400 for everything else this server refuses.
fn error(id: Option<Value>, failure: &Failure) -> Reply {
    let mut error = json!({"code": failure.code, "message": failure.message});
    if let (Some(data), Some(map)) = (&failure.data, error.as_object_mut()) {
        map.insert("data".into(), data.clone());
    }
    let mut body = json!({"jsonrpc": "2.0", "error": error});
    if let (Some(id), Some(map)) = (id, body.as_object_mut()) {
        map.insert("id".into(), id);
    }
    let status = if failure.code == codes::METHOD_NOT_FOUND {
        404
    } else {
        400
    };
    Reply {
        status,
        body: Some(body),
    }
}

#[cfg(test)]
mod tests {
    // The protocol's rules, without a transport, then each transport. The conformance suite checks the
    // same rules end to end in `crates/mcp-conformance`; these run everywhere.

    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;

    use serde_json::{Map, Value, json};

    use super::*;

    struct Two {
        tools: Vec<Tool>,
    }

    fn tool(name: &'static str, requires: &'static [&'static str]) -> Tool {
        Tool {
            name,
            title: name,
            description: "a test tool",
            input_schema: json!({"type": "object"}),
            output_schema: None,
            requires,
        }
    }

    impl Tools for Two {
        fn tools(&self) -> &[Tool] {
            &self.tools
        }

        fn call(
            &self,
            name: &str,
            arguments: &Map<String, Value>,
            progress: &mut Progress<'_>,
        ) -> Called {
            progress.report(1, Some(2), "half");
            match name {
                "echo" => Called::structured(Value::Object(arguments.clone())),
                _ => Called::text("sampled"),
            }
        }
    }

    fn server() -> Server<Two> {
        Server::new(
            Two {
                tools: vec![tool("echo", &[]), tool("sample", &["sampling"])],
            },
            "test",
            "1.2.3",
            "Test instructions.",
        )
    }

    fn meta() -> Value {
        json!({"io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION, "io.modelcontextprotocol/clientCapabilities": {}})
    }

    fn ask(method: &str, mut params: Value) -> (Reply, Vec<Value>) {
        if let Some(map) = params.as_object_mut() {
            map.entry("_meta").or_insert_with(meta);
        }
        let message = json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params});
        let mut notes = Vec::new();
        let reply = server().handle(message.to_string().as_bytes(), None, &mut |n| notes.push(n));
        (reply, notes)
    }

    fn code(reply: &Reply) -> Option<i64> {
        reply.body.as_ref()?.pointer("/error/code")?.as_i64()
    }

    #[test]
    fn discover_names_the_revision_tools_and_the_server_with_caching_hints() {
        let (reply, _) = ask("server/discover", json!({}));
        assert_eq!(reply.status, 200);
        let result = &reply.body.unwrap()["result"];
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["supportedVersions"], json!([PROTOCOL_VERSION]));
        assert_eq!(result["capabilities"], json!({"tools": {}}));
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"],
            json!({"name": "test", "version": "1.2.3"})
        );
        assert_eq!(result["ttlMs"], TTL_MS);
        assert_eq!(result["cacheScope"], "public");
    }

    #[test]
    fn tools_are_listed_in_order_and_a_cursor_never_given_out_is_refused() {
        let (reply, _) = ask("tools/list", json!({}));
        let result = &reply.body.unwrap()["result"];
        let names: Vec<&str> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["echo", "sample"]);
        assert_eq!(result["tools"][0]["inputSchema"], json!({"type": "object"}));
        assert_eq!(result["ttlMs"], TTL_MS);
        let (reply, _) = ask("tools/list", json!({"cursor": "x"}));
        assert_eq!(code(&reply), Some(codes::INVALID_PARAMS));
    }

    #[test]
    fn a_request_without_its_meta_is_invalid_params_with_status_400() {
        for meta in [
            json!(null),
            json!({"io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION}),
            json!({"io.modelcontextprotocol/clientCapabilities": {}}),
        ] {
            let mut params = json!({});
            if !meta.is_null() {
                params["_meta"] = meta;
            }
            let message =
                json!({"jsonrpc": "2.0", "id": "a", "method": "server/discover", "params": params});
            let reply = server().handle(message.to_string().as_bytes(), None, &mut |_| {});
            assert_eq!(
                (reply.status, code(&reply)),
                (400, Some(codes::INVALID_PARAMS))
            );
            assert_eq!(
                reply.body.unwrap()["id"],
                "a",
                "an error carries the request's id"
            );
        }
    }

    #[test]
    fn another_revision_is_refused_naming_the_one_supported_and_the_one_asked() {
        let (reply, _) = ask(
            "server/discover",
            json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2025-11-25", "io.modelcontextprotocol/clientCapabilities": {}}}),
        );
        assert_eq!(
            (reply.status, code(&reply)),
            (400, Some(codes::UNSUPPORTED_VERSION))
        );
        let data = &reply.body.unwrap()["error"]["data"];
        assert_eq!(data["supported"], json!([PROTOCOL_VERSION]));
        assert_eq!(data["requested"], "2025-11-25");
    }

    #[test]
    fn removed_and_unknown_methods_are_404_and_initialize_is_told_the_revision() {
        for method in [
            "ping",
            "logging/setLevel",
            "resources/subscribe",
            "prompts/list",
            "unknown/method",
        ] {
            let (reply, _) = ask(method, json!({}));
            assert_eq!(
                (reply.status, code(&reply)),
                (404, Some(codes::METHOD_NOT_FOUND)),
                "{method}"
            );
        }
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}});
        let reply = server().handle(message.to_string().as_bytes(), None, &mut |_| {});
        assert_eq!(
            (reply.status, code(&reply)),
            (404, Some(codes::METHOD_NOT_FOUND))
        );
        assert_eq!(
            reply.body.unwrap()["error"]["data"]["supported"],
            json!([PROTOCOL_VERSION])
        );
    }

    #[test]
    fn what_is_not_a_request_is_refused_without_an_id_and_a_notification_gets_no_answer() {
        let mut nothing = |_| {};
        let reply = server().handle(b"{not json", None, &mut nothing);
        assert_eq!(code(&reply), Some(codes::PARSE_ERROR));
        assert!(reply.body.unwrap().get("id").is_none());
        let reply = server().handle(b"[]", None, &mut nothing);
        assert_eq!(code(&reply), Some(codes::INVALID_REQUEST));
        let reply = server().handle(
            br#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#,
            None,
            &mut nothing,
        );
        assert_eq!(code(&reply), Some(codes::INVALID_REQUEST));
        let reply = server().handle(
            br#"{"jsonrpc":"1.0","id":1,"method":"tools/list"}"#,
            None,
            &mut nothing,
        );
        assert_eq!(code(&reply), Some(codes::INVALID_REQUEST));
        let reply = server().handle(
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#,
            None,
            &mut nothing,
        );
        assert_eq!(
            reply,
            Reply {
                status: 202,
                body: None
            }
        );
    }

    #[test]
    fn a_call_runs_the_tool_and_returns_its_content_and_structured_result() {
        let (reply, notes) = ask("tools/call", json!({"name": "echo", "arguments": {"a": 1}}));
        assert_eq!(reply.status, 200);
        let result = &reply.body.unwrap()["result"];
        assert_eq!(result["structuredContent"], json!({"a": 1}));
        assert_eq!(
            result["content"][0],
            json!({"type": "text", "text": "{\"a\":1}"})
        );
        assert_eq!(result["isError"], false);
        assert!(notes.is_empty(), "no progress without a progressToken");
    }

    #[test]
    fn progress_is_sent_only_to_a_request_that_asked_with_a_token() {
        let mut meta = meta();
        meta["progressToken"] = json!("p1");
        let (_, notes) = ask("tools/call", json!({"name": "echo", "_meta": meta}));
        assert_eq!(
            notes,
            [
                json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": "p1", "progress": 1, "message": "half", "total": 2}})
            ]
        );
    }

    #[test]
    fn an_unknown_tool_or_bad_arguments_are_invalid_params() {
        let (reply, _) = ask("tools/call", json!({"name": "nope"}));
        assert_eq!(
            (reply.status, code(&reply)),
            (400, Some(codes::INVALID_PARAMS))
        );
        let (reply, _) = ask("tools/call", json!({"name": "echo", "arguments": [1]}));
        assert_eq!(code(&reply), Some(codes::INVALID_PARAMS));
    }

    #[test]
    fn a_call_needing_a_capability_the_request_did_not_declare_is_refused_before_it_runs() {
        let (reply, notes) = ask("tools/call", json!({"name": "sample"}));
        assert_eq!(
            (reply.status, code(&reply)),
            (400, Some(codes::MISSING_CAPABILITY))
        );
        assert_eq!(
            reply.body.unwrap()["error"]["data"],
            json!({"requiredCapabilities": {"sampling": {}}})
        );
        assert!(notes.is_empty(), "the tool did not run");
        let declared = json!({"io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION, "io.modelcontextprotocol/clientCapabilities": {"sampling": {}}});
        let (reply, _) = ask("tools/call", json!({"name": "sample", "_meta": declared}));
        assert_eq!(
            reply.body.unwrap()["result"]["content"][0]["text"],
            "sampled"
        );
    }

    #[test]
    fn on_http_the_headers_must_match_the_body() {
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "echo", "_meta": meta()}});
        let good = Headers {
            protocol_version: Some(PROTOCOL_VERSION.into()),
            method: Some("tools/call".into()),
            name: Some("echo".into()),
        };
        let reply = server().handle(call.to_string().as_bytes(), Some(&good), &mut |_| {});
        assert_eq!(reply.status, 200);
        let wrong = [
            Headers {
                protocol_version: None,
                ..good.clone()
            },
            Headers {
                protocol_version: Some("2025-11-25".into()),
                ..good.clone()
            },
            Headers {
                method: Some("tools/list".into()),
                ..good.clone()
            },
            Headers {
                name: None,
                ..good.clone()
            },
            Headers {
                name: Some("sample".into()),
                ..good.clone()
            },
        ];
        for headers in wrong {
            let reply = server().handle(call.to_string().as_bytes(), Some(&headers), &mut |_| {});
            assert_eq!(
                (reply.status, code(&reply)),
                (400, Some(codes::HEADER_MISMATCH)),
                "{headers:?}"
            );
        }
    }

    #[test]
    fn stdio_answers_line_by_line_with_notifications_first_and_skips_an_oversized_line() {
        let mut meta = meta();
        meta["progressToken"] = json!(9);
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "echo", "_meta": meta}});
        let huge = "x".repeat(MAX_MESSAGE + 10);
        let input = format!(
            "\n{call}\n{huge}\n{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\"}}\n"
        );
        let mut output = Vec::new();
        stdio::serve(&server(), input.as_bytes(), &mut output).unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!(lines[0]["method"], "notifications/progress");
        assert_eq!(lines[1]["id"], 1);
        assert_eq!(lines[2]["error"]["code"], codes::INVALID_REQUEST);
    }

    #[test]
    fn stdio_takes_a_message_of_exactly_the_cap_and_never_loses_the_next_one() {
        let request = |id: u64| {
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {"_meta": meta()}})
                .to_string()
        };
        let pad = |text: String, to: usize| format!("{text}{}", " ".repeat(to - text.len()));
        let at_cap = pad(request(1), MAX_MESSAGE);
        let over_cap = pad(request(3), MAX_MESSAGE + 1);
        let input = format!("{at_cap}\n{}\n{over_cap}\n{}\n", request(2), request(4));
        let mut output = Vec::new();
        stdio::serve(&server(), input.as_bytes(), &mut output).unwrap();
        let ids: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap()["id"].clone())
            .collect();
        // The message over the cap is answered without an id (it was never read), and the one after it
        // is still answered.
        assert_eq!(ids, [json!(1), json!(2), Value::Null, json!(4)]);
    }

    fn http(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    fn post(port: u16, host: &str, extra: &str, body: &str) -> String {
        http(
            port,
            &format!(
                "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{extra}Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    #[test]
    fn http_serves_loopback_only_and_refuses_another_host_or_origin() {
        assert!(http::bind("0.0.0.0:0").unwrap_err().contains("loopback"));
        assert!(http::bind("localhost:1").is_err(), "an address, not a name");
        let listener = http::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || http::serve(listener, Arc::new(server())));
        let host = format!("localhost:{port}");
        let discover = json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta()}}).to_string();
        let headers =
            format!("MCP-Protocol-Version: {PROTOCOL_VERSION}\r\nMcp-Method: server/discover\r\n");
        let ok = post(port, &host, &headers, &discover);
        assert!(ok.starts_with("HTTP/1.1 200 OK"), "{ok}");
        assert!(ok.contains("\"supportedVersions\""), "{ok}");
        for (host, origin) in [
            ("evil.example:80", ""),
            (&host as &str, "Origin: http://evil.example\r\n"),
            ("localhost:1", ""),
        ] {
            let refused = post(port, host, &format!("{headers}{origin}"), &discover);
            assert!(
                refused.starts_with("HTTP/1.1 403"),
                "{host} {origin}: {refused}"
            );
        }
        let mine = post(
            port,
            &host,
            &format!("{headers}Origin: http://127.0.0.1:{port}\r\n"),
            &discover,
        );
        assert!(mine.starts_with("HTTP/1.1 200"), "{mine}");
        let get = http(port, &format!("GET /mcp HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(
            get.starts_with("HTTP/1.1 405") && get.contains("Allow: POST"),
            "{get}"
        );
        let elsewhere = http(
            port,
            &format!("POST /other HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\n\r\n"),
        );
        assert!(elsewhere.starts_with("HTTP/1.1 404"), "{elsewhere}");
        let notification = post(
            port,
            &host,
            "",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#,
        );
        assert!(notification.starts_with("HTTP/1.1 202"), "{notification}");
    }

    #[test]
    fn http_streams_progress_then_the_result_and_decodes_a_base64_name() {
        let listener = http::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || http::serve(listener, Arc::new(server())));
        let mut meta = meta();
        meta["progressToken"] = json!("t");
        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "echo", "_meta": meta}}).to_string();
        // "echo" in the revision's base64 form.
        let headers = format!(
            "MCP-Protocol-Version: {PROTOCOL_VERSION}\r\nMcp-Method: tools/call\r\nMcp-Name: =?base64?ZWNobw==?=\r\n"
        );
        let streamed = post(port, &format!("127.0.0.1:{port}"), &headers, &call);
        assert!(
            streamed.contains("Content-Type: text/event-stream"),
            "{streamed}"
        );
        let progress = streamed.find("notifications/progress").unwrap();
        let result = streamed.find("\"result\"").unwrap();
        assert!(progress < result, "{streamed}");
        let mismatch = post(
            port,
            &format!("127.0.0.1:{port}"),
            &headers.replace("ZWNobw==", "bm9wZQ=="),
            &call,
        );
        assert!(
            mismatch.starts_with("HTTP/1.1 400") && mismatch.contains("-32020"),
            "{mismatch}"
        );
    }
}
