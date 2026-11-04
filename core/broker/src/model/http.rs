//! Plain HTTP/1.1 to a model server on this host (ADR-0037).
//!
//! This carries the key in clear, so it reaches only a loopback address: a local OpenAI-compatible
//! server such as Ollama or vLLM. A provider on the internet needs TLS, which this build does not have.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::{Transport, TransportError, Upstream, UpstreamResponse};

const MAX_HEAD: usize = 16 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest silence allowed while a response streams.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Sends model calls to a loopback HTTP server.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    authority: String,
}

impl HttpTransport {
    /// `base` is `http://host[:port]` with no path, where host is `localhost`, `127.0.0.1` or `[::1]`.
    pub fn new(base: &str) -> Result<Self, TransportError> {
        if base.starts_with("https://") {
            return Err(TransportError("transport.tls_unavailable"));
        }
        let authority = base
            .strip_prefix("http://")
            .map(|a| a.trim_end_matches('/'))
            .filter(|a| !a.is_empty() && !a.contains(['/', '?', '#', '@']))
            .ok_or(TransportError("transport.bad_base"))?;
        let host = match authority.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => {
                port.parse::<u16>()
                    .map_err(|_| TransportError("transport.bad_base"))?;
                host
            }
            _ => authority,
        };
        if !["localhost", "127.0.0.1", "[::1]"].contains(&host) {
            return Err(TransportError("transport.not_loopback"));
        }
        let authority = if authority == host {
            format!("{host}:80")
        } else {
            authority.to_string()
        };
        Ok(Self { authority })
    }
}

fn clean(text: &str) -> bool {
    !text.bytes().any(|b| b.is_ascii_control() || b == b' ')
}

impl Transport for HttpTransport {
    fn send(&self, request: &Upstream) -> Result<UpstreamResponse, TransportError> {
        let bad = TransportError("transport.bad_request");
        if !request.target.starts_with('/') || !clean(&request.target) {
            return Err(bad);
        }
        let mut head = format!(
            "POST {} HTTP/1.1\r\nhost: {}\r\nconnection: close\r\naccept-encoding: identity\r\ncontent-length: {}\r\n",
            request.target,
            self.authority,
            request.body.len()
        );
        for (name, value) in &request.headers {
            if !clean(name) || value.bytes().any(|b| b.is_ascii_control()) {
                return Err(bad);
            }
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");

        let unreachable = |_| TransportError("transport.unreachable");
        let mut stream = TcpStream::connect(&self.authority).map_err(unreachable)?;
        stream
            .set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(unreachable)?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(unreachable)?;
        stream.write_all(head.as_bytes()).map_err(unreachable)?;
        stream.write_all(&request.body).map_err(unreachable)?;
        stream.flush().map_err(unreachable)?;
        read_response(BufReader::new(stream))
    }
}

fn read_response(mut reader: BufReader<TcpStream>) -> Result<UpstreamResponse, TransportError> {
    let protocol = TransportError("transport.bad_response");
    let mut lines = Vec::new();
    let mut read = 0;
    loop {
        let mut line = Vec::new();
        let n = (&mut reader)
            .take((MAX_HEAD - read) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| protocol)?;
        read += n;
        if n == 0 || !line.ends_with(b"\n") {
            return Err(protocol);
        }
        let line = String::from_utf8(line).map_err(|_| protocol)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        lines.push(line.to_string());
    }
    let mut lines = lines.into_iter();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or(protocol)?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            l.split_once(':')
                .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    let get = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    let body: Box<dyn Read + Send> =
        if get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
            Box::new(Chunked {
                inner: reader,
                left: 0,
                done: false,
            })
        } else if let Some(length) = get("content-length") {
            let length = length.parse::<u64>().map_err(|_| protocol)?;
            Box::new(reader.take(length))
        } else {
            Box::new(reader)
        };
    Ok(UpstreamResponse {
        status,
        headers,
        body,
    })
}

/// A chunked transfer-encoded body, decoded as it is read.
struct Chunked<R> {
    inner: R,
    left: u64,
    done: bool,
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let mut line = String::new();
            (&mut self.inner).take(256).read_line(&mut line)?;
            let size = line.trim().split(';').next().unwrap_or_default();
            self.left = u64::from_str_radix(size.trim(), 16).map_err(|_| invalid("chunk size"))?;
            if self.left == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let want = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= n as u64;
        if self.left == 0 {
            let mut end = [0u8; 2];
            self.inner.read_exact(&mut end)?;
            if &end != b"\r\n" {
                return Err(invalid("chunk end"));
            }
        }
        Ok(n)
    }
}
