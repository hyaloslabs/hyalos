//! One contract for every model adapter (S3.8, ADR-0016, ADR-0037): the key is added and the
//! placeholder dropped, tokens are metered per job and a spent budget is a 402, a reader's tool
//! definitions are refused, provider-hosted tools are removed from every request, a busy provider is
//! retried and then answered with the harness's retryable status, and a query string is accepted.

use std::collections::{BTreeSet, VecDeque};
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_broker::model::{
    Adapter, Anthropic, ApiKey, Meter, OpenAi, Relay, Request, Sleep, Transport, TransportError,
    Upstream, UpstreamResponse,
};
use hyalos_types::{Budget, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp};
use serde_json::{Value, json};

type Res = Result<(), Box<dyn std::error::Error>>;

const KEY: &str = "sk-real-provider-key-0123456789";
const PLACEHOLDER: &str = "placeholder-key";
/// A successful answer that reports a little usage.
const OK: &[u8] = br#"{"usage":{"input_tokens":1,"output_tokens":1}}"#;

type Headers = Vec<(String, String)>;
type Answer = Result<(u16, Headers, Vec<u8>), TransportError>;

/// A provider that answers from a script and records what it was sent.
#[derive(Default)]
struct Script {
    answers: Mutex<VecDeque<Answer>>,
    sent: Mutex<Vec<Upstream>>,
}

impl Script {
    fn answer(&self, status: u16, headers: &[(&str, &str)], body: &[u8]) {
        let headers = headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        self.lock_answers()
            .push_back(Ok((status, headers, body.to_vec())));
    }

    fn fail(&self) {
        self.fail_with("transport.unreachable");
    }

    fn fail_with(&self, code: &'static str) {
        self.lock_answers().push_back(Err(TransportError(code)));
    }

    fn lock_answers(&self) -> std::sync::MutexGuard<'_, VecDeque<Answer>> {
        self.answers.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sent(&self) -> Vec<(String, Headers, Value)> {
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|u| {
                (
                    u.target.clone(),
                    u.headers.clone(),
                    serde_json::from_slice(&u.body).unwrap_or(Value::Null),
                )
            })
            .collect()
    }
}

impl Transport for Script {
    fn send(&self, request: &Upstream) -> Result<UpstreamResponse, TransportError> {
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Upstream {
                target: request.target.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
            });
        let (status, headers, body) =
            self.lock_answers()
                .pop_front()
                .unwrap_or(Ok((200, Vec::new(), OK.to_vec())))?;
        Ok(UpstreamResponse {
            status,
            headers,
            body: Box::new(Cursor::new(body)),
        })
    }
}

#[derive(Default)]
struct Waits(Mutex<Vec<Duration>>);

impl Sleep for Waits {
    fn sleep(&self, duration: Duration) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(duration);
    }
}

impl Waits {
    fn all(&self) -> Vec<Duration> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// What the contract needs to know about one protocol.
struct Case {
    adapter: Arc<dyn Adapter>,
    /// The generation path the harness uses, with a query string.
    path: &'static str,
    count_path: &'static str,
    /// The header and value the real key must arrive in.
    credential: (&'static str, String),
    /// The header the harness's placeholder arrives in.
    placeholder: (&'static str, String),
    overloaded: u16,
    /// A tool the harness runs, and tools the provider would run.
    client_tool: Value,
    hosted: Vec<Value>,
    /// A tool choice that names the first hosted tool.
    hosted_choice: Value,
    /// A JSON response reporting `json_tokens`, and an event stream reporting `stream_tokens`.
    json_response: Value,
    json_tokens: u64,
    stream: &'static str,
    stream_tokens: u64,
}

fn anthropic() -> Case {
    Case {
        adapter: Arc::new(Anthropic),
        path: "/v1/messages?beta=true",
        count_path: "/v1/messages/count_tokens?beta=true",
        credential: ("x-api-key", KEY.into()),
        placeholder: ("x-api-key", PLACEHOLDER.into()),
        overloaded: 529,
        client_tool: json!({ "name": "Bash", "description": "run", "input_schema": { "type": "object" } }),
        hosted: vec![
            json!({ "type": "web_search_20250305", "name": "web_search", "max_uses": 5 }),
            json!({ "type": "web_fetch_20250910", "name": "web_fetch" }),
            json!({ "type": "code_execution_20250825", "name": "code_execution" }),
            json!({ "type": "mcp_toolset", "mcp_server_name": "remote" }),
        ],
        hosted_choice: json!({ "type": "tool", "name": "web_search" }),
        json_response: json!({
            "type": "message", "role": "assistant", "content": [],
            "usage": { "input_tokens": 100, "cache_read_input_tokens": 20, "output_tokens": 30 }
        }),
        json_tokens: 150,
        stream: concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":40,\"output_tokens\":1}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":12}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        ),
        stream_tokens: 52,
    }
}

fn openai() -> Case {
    Case {
        adapter: Arc::new(OpenAi),
        path: "/v1/responses?trace=0",
        count_path: "/v1/responses/input_tokens",
        credential: ("authorization", format!("Bearer {KEY}")),
        placeholder: ("Authorization", format!("Bearer {PLACEHOLDER}")),
        overloaded: 503,
        client_tool: json!({ "type": "function", "name": "shell", "parameters": { "type": "object" } }),
        hosted: vec![
            json!({ "type": "web_search" }),
            json!({ "type": "file_search", "vector_store_ids": ["vs_1"] }),
            json!({ "type": "code_interpreter", "container": { "type": "auto" } }),
            json!({ "type": "mcp", "server_label": "remote", "server_url": "https://example.com" }),
        ],
        hosted_choice: json!({ "type": "web_search" }),
        json_response: json!({
            "object": "response", "output": [],
            "usage": { "input_tokens": 90, "output_tokens": 60, "total_tokens": 150 }
        }),
        json_tokens: 150,
        stream: concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"usage\":null}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":40,\"output_tokens\":12}}}\n\n",
        ),
        stream_tokens: 52,
    }
}

fn manifest(job: u64, role: Role, tokens: u64) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(job),
        role,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens },
        deadline: Timestamp::from_millis(u64::MAX),
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

struct Rig {
    relay: Relay,
    script: Arc<Script>,
    meter: Arc<Meter>,
    waits: Arc<Waits>,
}

fn rig(case: &Case) -> Result<Rig, Box<dyn std::error::Error>> {
    let script = Arc::new(Script::default());
    let meter = Arc::new(Meter::default());
    let waits = Arc::new(Waits::default());
    let relay = Relay::new(
        Arc::clone(&case.adapter),
        script.clone(),
        ApiKey::new(KEY).ok_or("key")?,
        meter.clone(),
        waits.clone(),
    );
    Ok(Rig {
        relay,
        script,
        meter,
        waits,
    })
}

fn call(case: &Case, path: &str, body: &Value) -> Request {
    Request {
        method: "POST".into(),
        target: path.into(),
        headers: vec![
            (case.placeholder.0.into(), case.placeholder.1.clone()),
            ("Authorization".into(), format!("Bearer {PLACEHOLDER}")),
            ("X-Api-Key".into(), PLACEHOLDER.into()),
            ("anthropic-version".into(), "2023-06-01".into()),
            ("content-type".into(), "application/json".into()),
        ],
        body: serde_json::to_vec(body).unwrap_or_default(),
    }
}

fn plain() -> Value {
    json!({ "model": "m", "stream": false, "max_tokens": 64, "input": "hello", "messages": [] })
}

fn with_tools(tools: Vec<Value>) -> Value {
    let mut body = plain();
    body["tools"] = Value::Array(tools);
    body
}

/// The status of a response, read to the end as a harness would, so it is metered.
fn status(response: hyalos_broker::model::Response) -> u16 {
    let status = response.status;
    let _ = read_all(response.body);
    status
}

fn read_all(mut body: impl Read) -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    body.read_to_string(&mut text)?;
    Ok(text)
}

/// The contract. Every adapter must pass it.
fn contract(case: &Case) -> Res {
    key_is_added_and_placeholder_dropped(case)?;
    usage_is_metered_per_job(case)?;
    unread_usage_spends_the_budget(case)?;
    calls_in_flight_hold_their_output(case)?;
    a_malformed_request_is_refused_not_retried(case)?;
    a_spent_budget_is_402_naming_it(case)?;
    a_reader_may_send_no_tools(case)?;
    hosted_tools_are_removed_from_every_request(case)?;
    a_busy_provider_is_retried_then_answered_retryably(case)?;
    unknown_paths_and_methods_are_refused(case)?;
    Ok(())
}

fn key_is_added_and_placeholder_dropped(case: &Case) -> Res {
    let r = rig(case)?;
    r.script.answer(200, &[], OK);
    let response = r.relay.handle(
        &manifest(1, Role::Actor, 1000),
        call(case, case.path, &plain()),
    );
    assert_eq!(response.status, 200);
    let sent = r.script.sent();
    let (target, headers, _) = sent.first().ok_or("nothing sent")?;
    assert_eq!(target, case.path, "the query string is passed on");
    let credentials: Vec<_> = headers
        .iter()
        .filter(|(n, _)| n == "authorization" || n == "x-api-key")
        .collect();
    assert_eq!(
        credentials,
        vec![&(case.credential.0.to_string(), case.credential.1.clone())],
        "only the real key reaches the provider"
    );
    let raw = format!("{headers:?}");
    assert!(
        !raw.contains(PLACEHOLDER),
        "the placeholder never reaches the provider"
    );
    let debug = format!("{:?}", ApiKey::new(KEY).ok_or("key")?);
    assert!(!debug.contains(KEY), "the key is never printed");
    Ok(())
}

fn usage_is_metered_per_job(case: &Case) -> Res {
    let r = rig(case)?;
    let job = manifest(2, Role::Actor, 10_000);
    let other = manifest(3, Role::Actor, 10_000);

    r.script.answer(
        200,
        &[("content-type", "application/json")],
        &serde_json::to_vec(&case.json_response)?,
    );
    let response = r.relay.handle(&job, call(case, case.path, &plain()));
    assert_eq!(response.status, 200);
    read_all(response.body)?;
    assert_eq!(r.meter.spent(job.job), case.json_tokens);

    r.script.answer(
        200,
        &[("content-type", "text/event-stream; charset=utf-8")],
        case.stream.as_bytes(),
    );
    let response = r.relay.handle(&job, call(case, case.path, &plain()));
    let text = read_all(response.body)?;
    assert_eq!(
        text, case.stream,
        "the stream reaches the harness unchanged"
    );
    assert_eq!(
        r.meter.spent(job.job),
        case.json_tokens + case.stream_tokens
    );
    assert_eq!(r.meter.spent(other.job), 0, "another job is not charged");

    // Counting tokens spends none.
    r.script.answer(200, &[], br#"{"input_tokens": 999}"#);
    let response = r
        .relay
        .handle(&other, call(case, case.count_path, &plain()));
    assert_eq!(response.status, 200);
    read_all(response.body)?;
    assert_eq!(r.meter.spent(other.job), 0);

    // An event line far over a megabyte is still read for its usage.
    let big = manifest(30, Role::Actor, 10_000_000);
    let mut long = case
        .stream
        .replace("\"usage\"", "\"pad\":\"PAD\",\"usage\"");
    long = long.replace("PAD", &"x".repeat(2 * 1024 * 1024));
    r.script.answer(
        200,
        &[("content-type", "text/event-stream")],
        long.as_bytes(),
    );
    read_all(r.relay.handle(&big, call(case, case.path, &plain())).body)?;
    assert_eq!(r.meter.spent(big.job), case.stream_tokens);
    Ok(())
}

/// A call in flight holds the output it may produce, capped to what the budget has left, so calls
/// side by side cannot all pass it; one that finds the rest held is told to try again, not refused.
fn calls_in_flight_hold_their_output(case: &Case) -> Res {
    let r = rig(case)?;
    let field = case.adapter.output_limit();
    let job = manifest(60, Role::Actor, 100);
    let mut asks = plain();
    asks[field] = json!(70);

    let first = r.relay.handle(&job, call(case, case.path, &asks));
    assert_eq!(first.status, 200);
    assert_eq!(r.meter.held(job.job), 70);
    let second = r.relay.handle(&job, call(case, case.path, &asks));
    assert_eq!(second.status, 200);
    assert_eq!(r.meter.held(job.job), 100);
    let sent = r.script.sent();
    assert_eq!(sent[0].2[field], 70);
    assert_eq!(sent[1].2[field], 30, "capped to what was left");

    let third = r.relay.handle(&job, call(case, case.path, &asks));
    assert_eq!(third.status, case.overloaded, "the rest is held: try again");
    assert_eq!(
        third.reason.as_ref().map(|r| r.as_str()),
        Some("budget.held")
    );

    // Ending the calls releases the holds and charges what they reported (2 each).
    read_all(first.body)?;
    read_all(second.body)?;
    assert_eq!((r.meter.spent(job.job), r.meter.held(job.job)), (4, 0));

    // A call with no limit asks for no more than is left.
    let open = manifest(61, Role::Actor, 500);
    let mut body = plain();
    body.as_object_mut().ok_or("object")?.remove(field);
    read_all(r.relay.handle(&open, call(case, case.path, &body)).body)?;
    assert_eq!(r.script.sent().last().ok_or("sent")?.2[field], 500);

    // A refused or failed call holds nothing afterwards.
    r.script.answer(400, &[], b"{}");
    read_all(r.relay.handle(&open, call(case, case.path, &plain())).body)?;
    assert_eq!(r.meter.held(open.job), 0);
    Ok(())
}

/// Metering fails closed: a generation whose usage cannot be read in full spends the whole budget.
fn unread_usage_spends_the_budget(case: &Case) -> Res {
    let r = rig(case)?;
    let bodies: [(&str, &[u8]); 3] = [
        ("application/json", br#"{"output":[]}"#),
        ("application/json", b"{not json"),
        ("text/event-stream", b"data: {\"type\":\"ping\"}\n\n"),
    ];
    for (n, (content_type, body)) in bodies.into_iter().enumerate() {
        let job = manifest(40 + n as u64, Role::Actor, 777);
        r.script
            .answer(200, &[("content-type", content_type)], body);
        read_all(r.relay.handle(&job, call(case, case.path, &plain())).body)?;
        assert_eq!(r.meter.spent(job.job), 777, "{content_type} {body:?}");
    }

    // A stream the harness drops before its usage arrived.
    let job = manifest(43, Role::Actor, 555);
    r.script.answer(
        200,
        &[("content-type", "text/event-stream")],
        case.stream.as_bytes(),
    );
    drop(r.relay.handle(&job, call(case, case.path, &plain())));
    assert_eq!(r.meter.spent(job.job), 555);

    // A stream that stops before the event that ends it has reported only provisional usage
    // (Anthropic's message_start says one output token), whether the provider ends it early or the
    // harness drops it part way after reading that usage (#335).
    let cut = case.stream.rfind("event: ").ok_or("no last event")?;
    let short = &case.stream[..cut];
    let job = manifest(44, Role::Actor, 666);
    r.script.answer(
        200,
        &[("content-type", "text/event-stream")],
        short.as_bytes(),
    );
    read_all(r.relay.handle(&job, call(case, case.path, &plain())).body)?;
    assert_eq!(r.meter.spent(job.job), 666, "a stream that ended early");

    let job = manifest(45, Role::Actor, 444);
    r.script.answer(
        200,
        &[("content-type", "text/event-stream")],
        case.stream.as_bytes(),
    );
    let mut body = r.relay.handle(&job, call(case, case.path, &plain())).body;
    let mut part = vec![0; cut];
    body.read_exact(&mut part)?;
    drop(body);
    assert_eq!(
        r.meter.spent(job.job),
        444,
        "a stream dropped after its usage began"
    );
    Ok(())
}

/// A request the broker cannot pass on safely is refused, not presented as a busy provider.
fn a_malformed_request_is_refused_not_retried(case: &Case) -> Res {
    let r = rig(case)?;
    let actor = manifest(50, Role::Actor, 1000);
    let spaced = format!("{} HTTP/1.1", case.path);
    let mut tab = call(case, case.path, &plain());
    tab.headers.push(("accept".into(), "a\tb".into()));
    let mut background = plain();
    background["background"] = json!(true);
    for (request, code) in [
        (call(case, &spaced, &plain()), "model.bad_request"),
        (tab, "model.bad_request"),
        (
            call(case, case.path, &background),
            "model.background_refused",
        ),
    ] {
        let response = r.relay.handle(&actor, request);
        assert_eq!(response.status, 400);
        assert_eq!(response.reason.as_ref().map(|r| r.as_str()), Some(code));
    }
    assert!(r.script.sent().is_empty());
    assert!(r.waits.all().is_empty());

    // An answer that could not be read is not retried: the provider may have generated already.
    r.script.fail_with("transport.bad_response");
    let response = r.relay.handle(&actor, call(case, case.path, &plain()));
    assert_eq!(response.status, 502);
    assert_eq!(r.script.sent().len(), 1);
    assert!(r.waits.all().is_empty());
    Ok(())
}

fn a_spent_budget_is_402_naming_it(case: &Case) -> Res {
    let r = rig(case)?;
    let job = manifest(4, Role::Actor, 100);
    r.script.answer(
        200,
        &[("content-type", "application/json")],
        &serde_json::to_vec(&case.json_response)?,
    );
    read_all(r.relay.handle(&job, call(case, case.path, &plain())).body)?;
    assert!(r.meter.spent(job.job) >= 100);

    let response = r.relay.handle(&job, call(case, case.path, &plain()));
    assert_eq!(response.status, 402);
    assert_eq!(
        response.reason.as_ref().map(|r| r.as_str()),
        Some("budget.spent")
    );
    let body = read_all(response.body)?;
    assert!(
        body.contains("budget of 100 tokens"),
        "the message names the budget: {body}"
    );
    assert_eq!(
        r.script.sent().len(),
        1,
        "a refused call never reaches the provider"
    );
    Ok(())
}

fn a_reader_may_send_no_tools(case: &Case) -> Res {
    let r = rig(case)?;
    let reader = manifest(5, Role::Reader, 1000);
    for body in [
        with_tools(vec![case.client_tool.clone()]),
        with_tools(vec![case.hosted[0].clone()]),
        {
            let mut b = plain();
            b["mcp_servers"] =
                json!([{ "type": "url", "url": "https://example.com", "name": "x" }]);
            b
        },
    ] {
        let response = r.relay.handle(&reader, call(case, case.path, &body));
        assert_eq!(response.status, 403);
        assert_eq!(
            response.reason.as_ref().map(|r| r.as_str()),
            Some("model.reader_tools")
        );
    }
    assert!(
        r.script.sent().is_empty(),
        "nothing a reader was refused reached the provider"
    );

    let response = r
        .relay
        .handle(&reader, call(case, case.path, &with_tools(vec![])));
    assert_eq!(response.status, 200, "a reader with no tools is served");
    let response = r.relay.handle(
        &manifest(6, Role::Actor, 1000),
        call(case, case.path, &with_tools(vec![case.client_tool.clone()])),
    );
    assert_eq!(response.status, 200, "an actor may send tools");
    Ok(())
}

fn hosted_tools_are_removed_from_every_request(case: &Case) -> Res {
    let r = rig(case)?;
    let actor = manifest(7, Role::Actor, 10_000);

    let mut mixed = with_tools(
        std::iter::once(case.client_tool.clone())
            .chain(case.hosted.iter().cloned())
            .collect(),
    );
    mixed["tool_choice"] = case.hosted_choice.clone();
    mixed["mcp_servers"] = json!([{ "type": "url", "url": "https://example.com", "name": "x" }]);
    mixed["include"] = json!([
        "web_search_call.action.sources",
        "reasoning.encrypted_content"
    ]);
    assert_eq!(
        status(r.relay.handle(&actor, call(case, case.path, &mixed))),
        200
    );

    // A separate request for only a hosted tool, as Claude Code's web search makes.
    let mut alone = with_tools(vec![case.hosted[0].clone()]);
    alone["tool_choice"] = case.hosted_choice.clone();
    alone["parallel_tool_calls"] = json!(true);
    assert_eq!(
        status(r.relay.handle(&actor, call(case, case.path, &alone))),
        200
    );

    // The same on the counting path.
    assert_eq!(
        status(r.relay.handle(&actor, call(case, case.count_path, &mixed))),
        200
    );

    let sent = r.script.sent();
    assert_eq!(sent.len(), 3);
    for (_, _, body) in &sent {
        let text = body.to_string();
        for hosted in &case.hosted {
            let kind = hosted["type"].as_str().ok_or("type")?;
            assert!(!text.contains(kind), "{kind} reached the provider: {text}");
        }
        assert!(body.get("mcp_servers").is_none());
        assert!(
            body.get("tool_choice").is_none(),
            "a choice of a removed tool is removed"
        );
        assert_eq!(body["model"], "m", "the rest of the request is kept");
    }
    assert_eq!(sent[0].2["tools"], json!([case.client_tool]));
    assert_eq!(sent[2].2["tools"], json!([case.client_tool]));
    assert!(
        sent[1].2.get("tools").is_none(),
        "no empty tool list is sent"
    );
    assert!(sent[1].2.get("parallel_tool_calls").is_none());

    // A choice of a tool that stays is kept.
    let mut kept = with_tools(vec![case.client_tool.clone(), case.hosted[0].clone()]);
    let name = case.client_tool["name"].clone();
    kept["tool_choice"] = json!({ "type": "tool", "name": name });
    r.relay.handle(&actor, call(case, case.path, &kept));
    let sent = r.script.sent();
    let last = sent.last().ok_or("sent")?;
    assert_eq!(last.2["tool_choice"]["name"], case.client_tool["name"]);
    Ok(())
}

fn a_busy_provider_is_retried_then_answered_retryably(case: &Case) -> Res {
    let r = rig(case)?;
    let actor = manifest(8, Role::Actor, 10_000);

    r.script.answer(429, &[("retry-after", "3")], b"{}");
    r.script.answer(case.overloaded, &[], b"{}");
    r.script.fail();
    r.script.answer(200, &[], OK);
    let response = r.relay.handle(&actor, call(case, case.path, &plain()));
    assert_eq!(response.status, 200, "absorbed three busy answers");
    read_all(response.body)?;
    assert_eq!(
        r.waits.all(),
        vec![
            Duration::from_secs(3),
            Duration::from_secs(2),
            Duration::from_secs(4)
        ],
        "waits the provider's retry-after, else doubles"
    );

    for _ in 0..4 {
        r.script.answer(429, &[], b"{}");
    }
    let response = r.relay.handle(&actor, call(case, case.path, &plain()));
    assert_eq!(
        response.status, case.overloaded,
        "gives up with the harness's retryable status"
    );
    assert_ne!(response.status, 429);
    assert_eq!(
        response.reason.as_ref().map(|r| r.as_str()),
        Some("provider.overloaded")
    );
    assert_eq!(r.script.sent().len(), 8);

    // A request error is the provider's to explain, and is not retried.
    r.script.answer(400, &[], br#"{"error":"bad"}"#);
    let response = r.relay.handle(&actor, call(case, case.path, &plain()));
    assert_eq!(response.status, 400);
    assert_eq!(read_all(response.body)?, r#"{"error":"bad"}"#);
    assert_eq!(r.script.sent().len(), 9);
    Ok(())
}

fn unknown_paths_and_methods_are_refused(case: &Case) -> Res {
    let r = rig(case)?;
    let actor = manifest(9, Role::Actor, 1000);
    for (method, path) in [
        ("POST", "/v1/models"),
        ("GET", case.path),
        ("POST", "/v1/messagesx"),
        ("POST", "/v1/files"),
    ] {
        let mut request = call(case, path, &plain());
        request.method = method.into();
        let response = r.relay.handle(&actor, request);
        assert_eq!(response.status, 404, "{method} {path}");
    }
    let mut bad = call(case, case.path, &plain());
    bad.body = b"not json".to_vec();
    assert_eq!(status(r.relay.handle(&actor, bad)), 400);
    assert!(r.script.sent().is_empty());
    Ok(())
}

#[test]
fn anthropic_passes_the_contract() -> Res {
    contract(&anthropic())
}

#[test]
fn openai_passes_the_contract() -> Res {
    contract(&openai())
}

#[test]
fn refusals_are_in_each_protocols_error_shape() -> Res {
    for (case, check) in [(anthropic(), "/error/type"), (openai(), "/error/message")] {
        let r = rig(&case)?;
        let response = r.relay.handle(
            &manifest(10, Role::Reader, 1000),
            call(
                &case,
                case.path,
                &with_tools(vec![case.client_tool.clone()]),
            ),
        );
        let body: Value = serde_json::from_str(&read_all(response.body)?)?;
        assert!(body.pointer(check).is_some(), "{body}");
    }
    Ok(())
}

#[test]
fn openai_spent_quota_is_402_not_retried() -> Res {
    let case = openai();
    let r = rig(&case)?;
    r.script.answer(
        429,
        &[],
        br#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#,
    );
    let response = r.relay.handle(
        &manifest(11, Role::Actor, 1000),
        call(&case, case.path, &plain()),
    );
    assert_eq!(response.status, 402);
    assert_eq!(r.script.sent().len(), 1);
    assert!(r.waits.all().is_empty());
    Ok(())
}

#[test]
fn anthropic_client_tools_are_kept() -> Res {
    let case = anthropic();
    let r = rig(&case)?;
    let tools = vec![
        json!({ "type": "bash_20250124", "name": "bash" }),
        json!({ "type": "text_editor_20250728", "name": "str_replace_based_edit_tool" }),
        json!({ "type": "custom", "name": "x", "input_schema": {} }),
        json!({ "type": "something_new_20270101", "name": "unknown" }),
        json!({ "type": "bash_code_execution_20250825", "name": "server_bash" }),
    ];
    r.relay.handle(
        &manifest(12, Role::Actor, 1000),
        call(&case, case.path, &with_tools(tools)),
    );
    let sent = r.script.sent();
    let names: Vec<_> = sent[0].2["tools"]
        .as_array()
        .ok_or("tools")?
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["bash", "str_replace_based_edit_tool", "x"],
        "an unknown type is removed"
    );
    Ok(())
}

#[test]
fn anthropic_version_is_added_when_missing() -> Res {
    let case = anthropic();
    let r = rig(&case)?;
    let mut request = call(&case, case.path, &plain());
    request.headers.retain(|(n, _)| n != "anthropic-version");
    r.relay.handle(&manifest(13, Role::Actor, 1000), request);
    let sent = r.script.sent();
    assert!(
        sent[0]
            .1
            .contains(&("anthropic-version".into(), "2023-06-01".into()))
    );
    Ok(())
}
