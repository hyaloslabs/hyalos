//! Streamable HTTP on loopback (ADR-0010, ADR-0067). Off unless the operator asks for it with
//! `hyalos mcp --listen`, and only on a loopback address.
//!
//! One POST to `/mcp` per message, answered with JSON, or with a stream of server-sent events when
//! the call sends notifications before its result and the client accepts a stream. The connection is
//! closed after each answer. There is no session and no GET stream: GET and DELETE are 405. A Host or
//! Origin that is not this loopback address and port is 403, which is the defence against DNS
//! rebinding. There is no authentication yet: the driving agent's credential is S5.8's.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::{Headers, MAX_MESSAGE, Server, Tools};

/// The endpoint's path.
pub const PATH: &str = "/mcp";
/// Connections served at once; one more is answered 503 and closed.
pub const MAX_CONNECTIONS: usize = 32;
const MAX_HEAD: u64 = 16 * 1024;
const MAX_HEADERS: usize = 64;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Binds `addr`, which must be a loopback address (`127.0.0.1:<port>` or `[::1]:<port>`).
pub fn bind(addr: &str) -> Result<TcpListener, String> {
    let addr: SocketAddr = addr.parse().map_err(|_| {
        format!("--listen takes a loopback address and port such as 127.0.0.1:8765, not {addr:?}")
    })?;
    if !addr.ip().is_loopback() {
        return Err(format!(
            "--listen takes a loopback address only, so the server is not reachable from the network; {} is not one",
            addr.ip()
        ));
    }
    TcpListener::bind(addr).map_err(|e| format!("cannot listen on {addr}: {e}"))
}

/// Serves `listener` until it fails, one thread per connection.
pub fn serve<T: Tools + 'static>(listener: TcpListener, server: Arc<Server<T>>) -> io::Result<()> {
    let port = listener.local_addr()?.port();
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = respond(&mut stream, 503, None);
            continue;
        }
        let (server, active) = (Arc::clone(&server), Arc::clone(&active));
        std::thread::spawn(move || {
            let _ = connection(stream, &server, port);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

fn connection<T: Tools>(stream: TcpStream, server: &Server<T>, port: u16) -> io::Result<()> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let request = match read_head(&mut reader) {
        Ok(request) => request,
        Err(status) => return respond(&mut out, status, None),
    };
    let header = |name: &str| {
        request
            .headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    let origin_ok =
        header("origin").is_none_or(|o| o.strip_prefix("http://").is_some_and(|h| local(h, port)));
    if !header("host").is_some_and(|h| local(h, port)) || !origin_ok {
        return respond(&mut out, 403, None);
    }
    if request.target != PATH {
        return respond(&mut out, 404, None);
    }
    if request.method != "POST" {
        return respond(&mut out, 405, None);
    }
    if !header("content-type")
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("application/json"))
    {
        return respond(&mut out, 415, None);
    }
    if header("transfer-encoding").is_some() {
        return respond(&mut out, 411, None);
    }
    let Some(length) = header("content-length").and_then(|l| l.parse::<usize>().ok()) else {
        return respond(&mut out, 411, None);
    };
    if length > MAX_MESSAGE {
        return respond(&mut out, 413, None);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let headers = Headers {
        protocol_version: header("mcp-protocol-version").map(str::to_string),
        method: header("mcp-method").map(str::to_string),
        name: header("mcp-name").and_then(decode_name),
    };
    let streams = header("accept").is_some_and(|a| a.contains("text/event-stream"));
    let mut streaming = false;
    let mut failed = false;
    let reply = server.handle(&body, Some(&headers), &mut |note: Value| {
        if !streams || failed {
            return;
        }
        if !streaming {
            streaming = true;
            failed |= out.write_all(SSE_HEAD.as_bytes()).is_err();
        }
        failed |= event(&mut out, &note).is_err();
    });
    match (streaming, reply.body) {
        (true, Some(body)) => event(&mut out, &body),
        (true, None) => Ok(()),
        (false, body) => respond(&mut out, reply.status, body.as_ref()),
    }
}

const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";

fn event(out: &mut TcpStream, message: &Value) -> io::Result<()> {
    out.write_all(format!("event: message\ndata: {message}\n\n").as_bytes())?;
    out.flush()
}

struct Head {
    method: String,
    target: String,
    /// Names in lower case.
    headers: Vec<(String, String)>,
}

/// Reads the request line and headers, at most 16 KiB and 64 headers. The error is the status to
/// answer with.
fn read_head(reader: &mut BufReader<TcpStream>) -> Result<Head, u16> {
    let mut limited = reader.take(MAX_HEAD);
    let line = |limited: &mut io::Take<&mut BufReader<TcpStream>>| -> Result<String, u16> {
        let mut bytes = Vec::new();
        limited.read_until(b'\n', &mut bytes).map_err(|_| 400u16)?;
        if bytes.last() != Some(&b'\n') {
            return Err(431);
        }
        let text = String::from_utf8(bytes).map_err(|_| 400u16)?;
        Ok(text.trim_end_matches(['\r', '\n']).to_string())
    };
    let first = line(&mut limited)?;
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some("HTTP/1.1"), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(400);
    };
    let mut headers = Vec::new();
    loop {
        let text = line(&mut limited)?;
        if text.is_empty() {
            break;
        }
        if headers.len() == MAX_HEADERS {
            return Err(431);
        }
        let (name, value) = text.split_once(':').ok_or(400u16)?;
        if value.chars().any(|c| c.is_control() && c != '\t') {
            return Err(400);
        }
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok(Head {
        method: method.to_string(),
        target: target.to_string(),
        headers,
    })
}

/// Whether a Host value (or an Origin's after `http://`) names this server: localhost, 127.0.0.1 or
/// [::1], with this port.
fn local(host: &str, port: u16) -> bool {
    let Some((name, given)) = host.rsplit_once(':') else {
        return false;
    };
    matches!(name, "localhost" | "127.0.0.1" | "[::1]") && given == port.to_string()
}

/// `Mcp-Name` as given, or decoded from the revision's `=?base64?...?=` form. A value that does not
/// decode is none, which never matches the body.
fn decode_name(value: &str) -> Option<String> {
    let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|v| v.strip_suffix("?="))
    else {
        return Some(value.to_string());
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in encoded.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    String::from_utf8(out).ok()
}

fn respond(out: &mut TcpStream, status: u16, body: Option<&Value>) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        _ => "Service Unavailable",
    };
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if status == 405 {
        head.push_str("Allow: POST\r\n");
    }
    out.write_all(format!("{head}\r\n{body}").as_bytes())?;
    out.flush()
}
