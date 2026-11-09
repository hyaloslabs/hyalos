//! A local OpenAI-compatible server works through the OpenAI adapter over plain HTTP on loopback
//! (S3.8, ADR-0037), and plain HTTP reaches nothing else (TLS is in `model_tls.rs`, ADR-0039).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use hyalos_broker::model::{
    ApiKey, HttpTransport, Meter, OpenAi, Relay, Request, ThreadSleep, Transport, TransportError,
};
use hyalos_types::{Budget, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp};
use serde_json::{Value, json};

type Res = Result<(), Box<dyn std::error::Error>>;

const KEY: &str = "sk-local-0123456789";

/// What the server saw: the request line, lowercase headers and the body.
struct Seen {
    line: String,
    headers: Vec<(String, String)>,
    body: Value,
}

fn read_request(stream: &TcpStream) -> Result<Seen, Box<dyn std::error::Error + Send + Sync>> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((n, v)) = h.split_once(':') {
            headers.push((n.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let length: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Seen {
        line: line.trim_end().to_string(),
        headers,
        body: serde_json::from_slice(&body)?,
    })
}

type Served = thread::JoinHandle<Result<Vec<Seen>, Box<dyn std::error::Error + Send + Sync>>>;

/// Serves `responses` in turn, one connection each, and returns what each request was.
fn server(responses: Vec<Vec<u8>>) -> Result<(String, Served), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://{}", listener.local_addr()?);
    let handle = thread::spawn(move || {
        let mut seen = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept()?;
            seen.push(read_request(&stream)?);
            stream.write_all(&response)?;
        }
        Ok(seen)
    });
    Ok((base, handle))
}

fn chunked(head: &str, body: &str) -> Vec<u8> {
    let mut out = format!("{head}\r\ntransfer-encoding: chunked\r\n\r\n").into_bytes();
    // Split mid-line, so the scanner must join events across chunks.
    for part in body.as_bytes().chunks(17) {
        out.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

fn manifest() -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(21),
        role: Role::Actor,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 10_000 },
        deadline: Timestamp::from_millis(u64::MAX),
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

fn request(body: &Value) -> Request {
    Request {
        method: "POST".into(),
        target: "/v1/responses".into(),
        headers: vec![("authorization".into(), "Bearer placeholder".into())],
        body: serde_json::to_vec(body).unwrap_or_default(),
    }
}

#[test]
fn a_local_openai_compatible_server_works_through_the_openai_adapter() -> Res {
    let stream = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":31,\"output_tokens\":7}}}\n\n",
    );
    let json_body = r#"{"object":"response","usage":{"input_tokens":5,"output_tokens":5}}"#;
    let (base, server) = server(vec![
        chunked(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream",
            stream,
        ),
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{json_body}",
            json_body.len()
        )
        .into_bytes(),
    ])?;

    let meter = Arc::new(Meter::default());
    let relay = Relay::new(
        Arc::new(OpenAi),
        Arc::new(HttpTransport::new(&base)?),
        ApiKey::new(KEY).ok_or("key")?,
        meter.clone(),
        Arc::new(ThreadSleep),
    );
    let job = manifest();

    let call = json!({
        "model": "qwen3:8b", "stream": true, "input": "hi",
        "tools": [
            { "type": "function", "name": "shell", "parameters": { "type": "object" } },
            { "type": "web_search" }
        ]
    });
    let response = relay.handle(&job, request(&call));
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "text/event-stream");
    let mut text = String::new();
    { response.body }.read_to_string(&mut text)?;
    assert_eq!(text, stream);
    assert_eq!(meter.spent(job.job), 38);

    let response = relay.handle(
        &job,
        request(&json!({ "model": "qwen3:8b", "input": "hi" })),
    );
    let mut text = String::new();
    { response.body }.read_to_string(&mut text)?;
    assert_eq!(text, json_body);
    assert_eq!(meter.spent(job.job), 48);

    let seen = server
        .join()
        .map_err(|_| "server panicked")?
        .map_err(|e| e.to_string())?;
    assert_eq!(seen[0].line, "POST /v1/responses HTTP/1.1");
    let auth: Vec<_> = seen[0]
        .headers
        .iter()
        .filter(|(n, _)| n == "authorization")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(auth, [format!("Bearer {KEY}")]);
    assert_eq!(
        seen[0].body["tools"],
        json!([{ "type": "function", "name": "shell", "parameters": { "type": "object" } }])
    );
    Ok(())
}

#[test]
fn plain_http_reaches_only_loopback_and_anything_else_needs_tls() {
    for (base, code) in [
        ("https://", "transport.bad_base"),
        ("https://api.openai.com/v1", "transport.bad_base"),
        ("https://user@api.openai.com", "transport.bad_base"),
        ("https://api.openai.com:https", "transport.bad_base"),
        ("http://api.openai.com", "transport.not_loopback"),
        ("http://10.0.0.5:8000", "transport.not_loopback"),
        ("http://127.0.0.1:8000/v1", "transport.bad_base"),
        ("http://user@127.0.0.1:8000", "transport.bad_base"),
        ("http://127.0.0.1:port", "transport.bad_base"),
        ("ftp://127.0.0.1", "transport.bad_base"),
    ] {
        assert_eq!(
            HttpTransport::new(base).err(),
            Some(TransportError(code)),
            "{base}"
        );
    }
    for base in [
        "http://127.0.0.1:11434",
        "http://localhost:8000/",
        "http://[::1]:8000",
        "http://127.0.0.1",
        "https://api.openai.com",
        "https://api.anthropic.com:443/",
        "https://10.0.0.5:8443",
    ] {
        assert!(HttpTransport::new(base).is_ok(), "{base}");
    }
}

#[test]
fn a_header_that_would_split_the_request_is_refused() -> Res {
    let transport = HttpTransport::new("http://127.0.0.1:9")?;
    let request = hyalos_broker::model::Upstream {
        target: "/v1/responses".into(),
        headers: vec![("accept".into(), "a\r\nx-evil: 1".into())],
        body: Vec::new(),
    };
    assert_eq!(
        transport.send(&request).err(),
        Some(TransportError("transport.bad_request"))
    );
    let request = hyalos_broker::model::Upstream {
        target: "/v1/responses?x HTTP/1.1".into(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    assert_eq!(
        transport.send(&request).err(),
        Some(TransportError("transport.bad_request"))
    );
    Ok(())
}

#[test]
fn an_unreachable_server_is_answered_retryably() -> Res {
    // Bind and drop, so nothing listens on the port.
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let relay = Relay::new(
        Arc::new(OpenAi),
        Arc::new(HttpTransport::new(&format!("http://127.0.0.1:{port}"))?),
        ApiKey::new(KEY).ok_or("key")?,
        Arc::new(Meter::default()),
        Arc::new(ThreadSleep),
    )
    .with_backoff(hyalos_broker::model::Backoff {
        attempts: 2,
        base: std::time::Duration::from_millis(1),
        cap: std::time::Duration::from_millis(1),
    });
    let response = relay.handle(
        &manifest(),
        request(&json!({ "model": "m", "input": "hi" })),
    );
    assert_eq!(response.status, 503);
    Ok(())
}
