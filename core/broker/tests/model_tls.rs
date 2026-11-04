//! The broker reaches a provider over TLS, with the certificate checked before the key is sent
//! (S3.15b, ADR-0039).
//!
//! The server here presents a certificate for `localhost` from a test authority in
//! `tests/fixtures/tls` (100-year validity; the authority's private key was discarded when it was
//! made, so nothing else can be issued from it). Only a transport told to trust that authority, in
//! code, accepts it; the Mozilla roots every real transport uses refuse it.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use hyalos_broker::model::{
    ApiKey, HttpTransport, Meter, OpenAi, Relay, Request, ThreadSleep, Transport, TransportError,
    Upstream,
};
use hyalos_types::{Budget, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};

type Res = Result<(), Box<dyn std::error::Error>>;
type ServerRes<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const KEY: &str = "sk-tls-0123456789";
const CA: &[u8] = include_bytes!("fixtures/tls/ca.pem");
const LEAF: &[u8] = include_bytes!("fixtures/tls/leaf.pem");
const LEAF_KEY: &[u8] = include_bytes!("fixtures/tls/leaf.key");

fn test_authority() -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error>> {
    Ok(vec![CertificateDer::from_pem_slice(CA)?])
}

fn server_config() -> ServerRes<Arc<ServerConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from_pem_slice(LEAF)?],
            PrivateKeyDer::from_pem_slice(LEAF_KEY)?,
        )?;
    Ok(Arc::new(config))
}

/// What the server saw: lowercase headers and the body, or that the handshake failed.
#[derive(Debug)]
enum Seen {
    Request {
        headers: Vec<(String, String)>,
        body: Value,
    },
    HandshakeFailed,
}

/// A connection that keeps every raw byte it reads, so a test can show what crossed the wire.
struct Recorded {
    tcp: TcpStream,
    raw: Vec<u8>,
}

impl Read for Recorded {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.tcp.read(buf)?;
        self.raw.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

impl Write for Recorded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tcp.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp.flush()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn read_request(stream: impl Read) -> ServerRes<Seen> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return Ok(Seen::HandshakeFailed);
    }
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
    Ok(Seen::Request {
        headers,
        body: serde_json::from_slice(&body)?,
    })
}

/// Each connection: what it was, and every raw byte the server received on it.
type Connections = Vec<(Seen, Vec<u8>)>;
type Served = thread::JoinHandle<ServerRes<Connections>>;

/// A TLS server on loopback that answers each connection with the next of `responses`, and returns
/// what each connection was. It returns the port.
fn server(responses: Vec<Vec<u8>>) -> Result<(u16, Served), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let handle = thread::spawn(move || -> ServerRes<Connections> {
        let config = server_config()?;
        let mut seen = Vec::new();
        for response in responses {
            let (tcp, _) = listener.accept()?;
            let tcp = Recorded {
                tcp,
                raw: Vec::new(),
            };
            let mut tls = StreamOwned::new(ServerConnection::new(Arc::clone(&config))?, tcp);
            let request = read_request(&mut tls)?;
            if matches!(request, Seen::Request { .. }) {
                tls.write_all(&response)?;
                tls.conn.send_close_notify();
                tls.flush()?;
            }
            if matches!(request, Seen::HandshakeFailed) {
                // Whatever the client sends after failing, keep it too.
                let mut rest = Vec::new();
                let _ = tls.sock.read_to_end(&mut rest);
            }
            seen.push((request, std::mem::take(&mut tls.sock.raw)));
        }
        Ok(seen)
    });
    Ok((port, handle))
}

fn joined(server: Served) -> Result<Connections, Box<dyn std::error::Error>> {
    Ok(server
        .join()
        .map_err(|_| "server panicked")?
        .map_err(|e| e.to_string())?)
}

fn chunked(head: &str, body: &str) -> Vec<u8> {
    let mut out = format!("{head}\r\ntransfer-encoding: chunked\r\n\r\n").into_bytes();
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
        job: JobId::from_seq(31),
        role: Role::Actor,
        tainted: false,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        actions: BTreeSet::new(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget { tokens: 10_000 },
        deadline: Timestamp::from_millis(u64::MAX),
        approval: None,
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

fn relay(transport: HttpTransport, meter: Arc<Meter>) -> Result<Relay, Box<dyn std::error::Error>> {
    Ok(Relay::new(
        Arc::new(OpenAi),
        Arc::new(transport),
        ApiKey::new(KEY).ok_or("key")?,
        meter,
        Arc::new(ThreadSleep),
    ))
}

#[test]
fn a_provider_over_tls_streams_and_answers_like_a_local_one() -> Res {
    let stream = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":31,\"output_tokens\":7}}}\n\n",
    );
    let json_body = r#"{"object":"response","usage":{"input_tokens":5,"output_tokens":5}}"#;
    let (port, server) = server(vec![
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
    let transport =
        HttpTransport::with_roots_der(&format!("https://localhost:{port}"), &test_authority()?)?;
    let meter = Arc::new(Meter::default());
    let relay = relay(transport, meter.clone())?;
    let job = manifest();

    let call = json!({
        "model": "m", "stream": true, "input": "hi",
        "tools": [{ "type": "web_search" }]
    });
    let response = relay.handle(&job, request(&call));
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "text/event-stream");
    let mut text = String::new();
    { response.body }.read_to_string(&mut text)?;
    assert_eq!(text, stream);
    assert_eq!(meter.spent(job.job), 38);

    let response = relay.handle(&job, request(&json!({ "model": "m", "input": "hi" })));
    let mut text = String::new();
    { response.body }.read_to_string(&mut text)?;
    assert_eq!(text, json_body);
    assert_eq!(meter.spent(job.job), 48);

    let seen = joined(server)?;
    let (Seen::Request { headers, body }, raw) = &seen[0] else {
        return Err(format!("{seen:?}").into());
    };
    assert!(headers.contains(&("authorization".into(), format!("Bearer {KEY}"))));
    assert!(headers.contains(&("host".into(), format!("localhost:{port}"))));
    assert!(
        !contains(raw, KEY.as_bytes()),
        "the key crossed the wire only encrypted"
    );
    assert!(
        !body.to_string().contains("web_search"),
        "hosted tools are removed over TLS too: {body}"
    );
    Ok(())
}

#[test]
fn a_certificate_the_mozilla_roots_do_not_vouch_for_is_refused_before_the_key_is_sent() -> Res {
    let (port, server) = server(vec![Vec::new()])?;
    // `new` is what the broker uses: the compiled-in Mozilla roots, which never include the test's.
    let relay = relay(
        HttpTransport::new(&format!("https://localhost:{port}"))?,
        Arc::new(Meter::default()),
    )?;
    let response = relay.handle(
        &manifest(),
        request(&json!({ "model": "m", "input": "hi" })),
    );
    assert_eq!(response.status, 502, "refused at once, not retried");
    assert_eq!(
        response.reason.map(|c| c.as_str().to_string()),
        Some("provider.tls_refused".into())
    );
    refused_without_the_key(server)
}

#[test]
fn a_certificate_for_another_name_is_refused() -> Res {
    let (port, server) = server(vec![Vec::new()])?;
    // The certificate names `localhost` only, so the same server by address is someone else.
    let transport =
        HttpTransport::with_roots_der(&format!("https://127.0.0.1:{port}"), &test_authority()?)?;
    let upstream = Upstream {
        target: "/v1/responses".into(),
        headers: vec![("authorization".into(), format!("Bearer {KEY}"))],
        body: b"{}".to_vec(),
    };
    assert_eq!(
        transport.send(&upstream).err(),
        Some(TransportError::TLS_REFUSED)
    );
    refused_without_the_key(server)
}

/// The one connection failed its handshake, and the key is nowhere in what the server received.
fn refused_without_the_key(server: Served) -> Res {
    let seen = joined(server)?;
    let [(Seen::HandshakeFailed, raw)] = &seen[..] else {
        return Err(format!("{seen:?}").into());
    };
    assert!(!raw.is_empty(), "the server saw the client's hello");
    assert!(!contains(raw, KEY.as_bytes()), "the key was sent");
    Ok(())
}

#[test]
fn a_server_that_is_not_there_is_unreachable_and_one_that_is_not_tls_is_refused() -> Res {
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let transport =
        HttpTransport::with_roots_der(&format!("https://localhost:{port}"), &test_authority()?)?;
    let upstream = Upstream {
        target: "/v1/responses".into(),
        headers: vec![("authorization".into(), format!("Bearer {KEY}"))],
        body: b"{}".to_vec(),
    };
    assert_eq!(
        transport.send(&upstream).err(),
        Some(TransportError::UNREACHABLE)
    );

    // A plain HTTP server answering a TLS hello.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let plain = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let (mut stream, _): (TcpStream, _) = listener.accept()?;
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n")?;
        let mut received = Vec::new();
        stream.read_to_end(&mut received)?;
        Ok(received)
    });
    let transport =
        HttpTransport::with_roots_der(&format!("https://localhost:{port}"), &test_authority()?)?;
    assert_eq!(
        transport.send(&upstream).err(),
        Some(TransportError::TLS_REFUSED)
    );
    let received = plain.join().map_err(|_| "panicked")??;
    assert!(!received.is_empty() && !contains(&received, KEY.as_bytes()));
    Ok(())
}
