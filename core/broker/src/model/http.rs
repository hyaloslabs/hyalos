//! HTTP/1.1 to a model provider (ADR-0037, ADR-0039).
//!
//! An `https://` base is reached over TLS (rustls), checked against the Mozilla roots compiled into
//! the binary; nothing in configuration can turn the check off. An `http://` base carries the key in
//! clear, so it reaches only a loopback address: a local server such as Ollama or vLLM.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use super::{Transport, TransportError, Upstream, UpstreamResponse};

const MAX_HEAD: usize = 16 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest a connection or a TLS handshake may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest silence allowed while a response streams.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Sends model calls to a provider over TLS, or to a loopback server over plain HTTP.
#[derive(Clone)]
pub struct HttpTransport {
    authority: String,
    tls: Option<(Arc<ClientConfig>, ServerName<'static>)>,
}

impl std::fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let scheme = if self.tls.is_some() { "https" } else { "http" };
        write!(f, "HttpTransport({scheme}://{})", self.authority)
    }
}

impl HttpTransport {
    /// `base` is `https://host[:port]` or `http://host[:port]` with no path; a plain `http` host must
    /// be `localhost`, `127.0.0.1` or `[::1]`.
    pub fn new(base: &str) -> Result<Self, TransportError> {
        Self::with_roots(
            base,
            webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect(),
        )
    }

    /// As [`HttpTransport::new`], trusting only the certificate authorities in `roots`, for a test's
    /// own authority. Code chooses this; nothing read from a file or the environment reaches it.
    pub fn with_roots_der(
        base: &str,
        roots: &[CertificateDer<'_>],
    ) -> Result<Self, TransportError> {
        let mut store = RootCertStore::empty();
        for root in roots {
            store
                .add(root.clone().into_owned())
                .map_err(|_| TransportError("transport.bad_root"))?;
        }
        Self::with_roots(base, store)
    }

    fn with_roots(base: &str, roots: RootCertStore) -> Result<Self, TransportError> {
        let bad = TransportError("transport.bad_base");
        let (authority, tls) = match base.split_once("://") {
            Some(("https", rest)) => (rest, true),
            Some(("http", rest)) => (rest, false),
            _ => return Err(bad),
        };
        let authority = Some(authority.trim_end_matches('/'))
            .filter(|a| !a.is_empty() && !a.contains(['/', '?', '#', '@']))
            .filter(|a| clean(a))
            .ok_or(bad)?;
        let host = match authority.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => {
                port.parse::<u16>()
                    .map_err(|_| TransportError("transport.bad_base"))?;
                host
            }
            _ => authority,
        };
        if !tls && !["localhost", "127.0.0.1", "[::1]"].contains(&host) {
            return Err(TransportError("transport.not_loopback"));
        }
        let authority = match (authority == host, tls) {
            (true, true) => format!("{host}:443"),
            (true, false) => format!("{host}:80"),
            (false, _) => authority.to_string(),
        };
        let tls = if tls {
            let name = host.trim_start_matches('[').trim_end_matches(']');
            let name = ServerName::try_from(name.to_string()).map_err(|_| bad)?;
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|_| TransportError("transport.tls_config"))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Some((Arc::new(config), name))
        } else {
            None
        };
        Ok(Self { authority, tls })
    }
}

/// Connects to the first address of `authority` that answers within the connect timeout.
fn connect(authority: &str) -> Option<TcpStream> {
    use std::net::ToSocketAddrs;
    authority
        .to_socket_addrs()
        .ok()?
        .find_map(|addr| TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok())
}

/// A connection the request is written to and the response read from.
trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

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

        let unreachable = |_| TransportError::UNREACHABLE;
        let mut tcp = connect(&self.authority).ok_or(TransportError::UNREACHABLE)?;
        tcp.set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(unreachable)?;
        tcp.set_read_timeout(Some(CONNECT_TIMEOUT))
            .map_err(unreachable)?;
        let mut stream: Box<dyn Stream> = match &self.tls {
            None => {
                tcp.set_read_timeout(Some(READ_TIMEOUT))
                    .map_err(unreachable)?;
                Box::new(tcp)
            }
            Some((config, name)) => {
                let mut tls = ClientConnection::new(Arc::clone(config), name.clone())
                    .map_err(|_| TransportError::TLS_REFUSED)?;
                // The handshake completes, and the certificate is checked, before the key is sent.
                // A TLS failure is refused; a network one sent nothing and may be tried again.
                while tls.is_handshaking() {
                    tls.complete_io(&mut tcp).map_err(|e| {
                        match e.get_ref().is_some_and(|inner| inner.is::<rustls::Error>()) {
                            true => TransportError::TLS_REFUSED,
                            false => TransportError::UNREACHABLE,
                        }
                    })?;
                }
                tcp.set_read_timeout(Some(READ_TIMEOUT))
                    .map_err(unreachable)?;
                Box::new(StreamOwned::new(tls, tcp))
            }
        };
        stream.write_all(head.as_bytes()).map_err(unreachable)?;
        stream.write_all(&request.body).map_err(unreachable)?;
        stream.flush().map_err(unreachable)?;
        read_response(BufReader::new(stream))
    }
}

fn read_response(
    mut reader: BufReader<Box<dyn Stream>>,
) -> Result<UpstreamResponse, TransportError> {
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
