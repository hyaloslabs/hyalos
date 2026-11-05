//! The egress gate and the allowlisting proxy (ADR-0009, ADR-0016, ADR-0042).
//!
//! A harness's traffic other than its model calls arrives on the job's socket as a proxy request: a
//! `CONNECT host:443` for TLS, or a plain-HTTP request in absolute form (`GET http://host/path`). The
//! broker passes it on only if the host is one of the manifest's egress hosts, on the usual port for
//! the request, and resolves to a public address. Everything else is refused with 403 and reported to
//! the engine as the egress gate's `deny`, carrying the SHA-256 of `host:port` (or of the target as
//! asked, when it is not one), never the name itself. A CONNECT tunnel carries the harness's own TLS:
//! the broker sees neither the content nor any credential in it.

use std::collections::BTreeSet;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyalos_types::wire::ToEngine;
use hyalos_types::{Answer, Digest, Gate, JobId, Manifest, ReasonCode};
use sha2::{Digest as _, Sha256};

use crate::model::Response;

/// The only port a CONNECT may name.
pub const TLS_PORT: u16 = 443;
/// The only port a plain-HTTP request may name.
pub const HTTP_PORT: u16 = 80;
/// Model providers' API hosts, never reached through the proxy even when a manifest lists them: a
/// prompt-injected harness could use one with an attacker's own key (ADR-0016). Model calls go
/// through the relay, with the broker's key. Every name under one is refused too, and so are the hosts
/// of the bases the broker's providers are reached at ([`Egress::refusing_bases`], ADR-0051).
pub const NEVER: [&str; 2] = ["api.anthropic.com", "api.openai.com"];
/// Refusals reported for one job. After this many distinct ones, one more says the rest were not.
pub const MAX_REPORTS: usize = 32;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How often a tunnel looks for its job ending.
const POLL: Duration = Duration::from_millis(500);
/// A tunnel or response that moves nothing for this long is closed.
const IDLE: Duration = Duration::from_secs(300);

/// Finds the addresses to connect to for an allowed host.
pub trait Resolve: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

/// The system's resolver, keeping only public addresses: a name that resolves to loopback, a private
/// network, link-local (cloud metadata) or any other non-public range is not reached through it. This
/// is the resolver the broker runs with; nothing in configuration replaces it.
#[derive(Debug, Clone, Copy, Default)]
pub struct PublicResolver;

impl Resolve for PublicResolver {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        // The absolute name, so the resolver's search domains cannot turn it into another host.
        Ok((format!("{host}."), port)
            .to_socket_addrs()?
            .filter(|a| is_public(a.ip()))
            .collect())
    }
}

/// Whether `ip` is a public unicast address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_v4(v4);
            }
            let s = v6.segments();
            // Global unicast is 2000::/3; within it, refuse documentation, 6to4, Teredo and ORCHID,
            // which carry or stand for other addresses.
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && s[1] == 0x0db8)
                && (s[0] & 0xfff0) != 0x3ff0
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && s[1] < 0x0200)
                && v6 != Ipv6Addr::UNSPECIFIED
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
}

/// How the broker reaches allowed hosts, and which provider hosts it never reaches.
#[derive(Clone)]
pub struct Egress {
    resolver: Arc<dyn Resolve>,
    /// [`NEVER`] and the hosts of the providers' bases, in the form [`host_name`] gives.
    providers: BTreeSet<String>,
}

impl std::fmt::Debug for Egress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Egress")
    }
}

impl Default for Egress {
    fn default() -> Self {
        Self::public()
    }
}

impl Egress {
    /// Through [`PublicResolver`].
    pub fn public() -> Self {
        Self::with_resolver(Arc::new(PublicResolver))
    }

    /// Through another resolver. For tests, which reach a server on loopback under a made-up name;
    /// called only from code.
    pub fn with_resolver(resolver: Arc<dyn Resolve>) -> Self {
        Self {
            resolver,
            providers: NEVER.iter().map(|h| h.to_string()).collect(),
        }
    }

    /// Also refuses the host of each provider base, such as an operator's gateway in `anthropic.base`,
    /// and every name under it. A base whose host is an address adds nothing: an address is never a
    /// destination. This only ever refuses more.
    pub fn refusing_bases<'a>(mut self, bases: impl IntoIterator<Item = &'a str>) -> Self {
        self.providers
            .extend(bases.into_iter().filter_map(base_host));
        self
    }

    /// Whether `host` (as [`host_name`] gives it) is a provider's API host or a name under one.
    pub fn is_provider_host(&self, host: &str) -> bool {
        self.providers.iter().any(|p| {
            host.strip_suffix(p.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
        })
    }
}

/// The host of a provider base such as `https://host[:port]`, in the form [`host_name`] gives, or
/// `None` when it has no host name (an address, or not a base at all).
pub fn base_host(base: &str) -> Option<String> {
    let (_, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => authority,
    };
    host_name(host).ok()
}

/// The refusals one job has reported, so a harness asking again and again fills neither the
/// channel nor the log.
#[derive(Debug)]
pub struct Reports {
    job: JobId,
    gate: Gate,
    /// The reason the report after the last one carries.
    too_many: &'static str,
    to: Option<Sender<ToEngine>>,
    seen: Mutex<(BTreeSet<Digest>, bool)>,
}

impl Reports {
    /// The egress gate's reports.
    pub fn new(job: JobId, to: Option<Sender<ToEngine>>) -> Self {
        Self::for_gate(job, Gate::Egress, "egress.too_many_refusals", to)
    }

    /// Another gate's reports, capped the same way: after [`MAX_REPORTS`], one with `too_many`.
    pub fn for_gate(
        job: JobId,
        gate: Gate,
        too_many: &'static str,
        to: Option<Sender<ToEngine>>,
    ) -> Self {
        Self {
            job,
            gate,
            too_many,
            to,
            seen: Mutex::new((BTreeSet::new(), false)),
        }
    }

    /// Reports this gate's `deny` for `inputs` once per job, up to [`MAX_REPORTS`].
    pub(crate) fn deny(&self, reason: &str, inputs: Digest, ended: &AtomicBool) {
        self.report(Answer::Deny, reason, inputs, ended);
    }

    /// Reports that this gate could not answer for `inputs`, which holds the job (ADR-0020). Counted
    /// with the denials.
    pub(crate) fn unknown(&self, reason: &str, inputs: Digest, ended: &AtomicBool) {
        self.report(Answer::Unknown, reason, inputs, ended);
    }

    fn report(&self, answer: Answer, reason: &str, inputs: Digest, ended: &AtomicBool) {
        // A refusal made as the job ends is not reported after its end.
        if ended.load(Ordering::SeqCst) {
            return;
        }
        let (Some(to), Some(code)) = (&self.to, ReasonCode::new(reason)) else {
            return;
        };
        let Ok(mut seen) = self.seen.lock() else {
            return;
        };
        let (reported, capped) = &mut *seen;
        let reason = if reported.contains(&inputs) || *capped {
            return;
        } else if reported.len() < MAX_REPORTS {
            reported.insert(inputs);
            code
        } else {
            *capped = true;
            code_of(self.too_many)
        };
        let _ = to.send(ToEngine::Gate {
            job: self.job,
            gate: self.gate,
            inputs,
            answer,
            reason,
        });
    }
}

fn code_of(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

/// The digest an egress report carries: SHA-256 of `host:port` as the broker understood it, or of
/// the target as asked when it is not a host and port.
pub fn inputs_digest(text: &str) -> Digest {
    Digest::from_bytes(Sha256::digest(text.as_bytes()).into())
}

/// Where one job's refusals on the forge are reported: the git proxy's and the forge API's.
pub(crate) struct Gates<'a> {
    pub manifest: &'a Reports,
    pub content: &'a Reports,
}

/// A proxy request, as read from the harness.
#[derive(Debug)]
pub struct ProxyRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
}

/// Where a proxy request is for, once the gate has read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub host: String,
    pub port: u16,
    /// For plain HTTP, the path and query to ask for.
    pub path: Option<String>,
}

/// Reads a proxy request's target. `Err` is the reason it is refused.
pub fn destination(method: &str, target: &str) -> Result<Destination, &'static str> {
    let (authority, path, default_port) = if method == "CONNECT" {
        (target, None, None)
    } else {
        let rest = target.strip_prefix("http://").ok_or("egress.bad_target")?;
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let path = match &rest[end..] {
            "" => "/".to_string(),
            p if p.starts_with('?') => format!("/{p}"),
            p => p.to_string(),
        };
        (&rest[..end], Some(path), Some(HTTP_PORT))
    };
    if authority.contains('@') || authority.starts_with('[') {
        return Err(if authority.starts_with('[') {
            "egress.ip_literal"
        } else {
            "egress.bad_target"
        });
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| "egress.bad_target")?),
        None => (authority, default_port.ok_or("egress.bad_target")?),
    };
    let host = host_name(host)?;
    let wanted = if path.is_some() { HTTP_PORT } else { TLS_PORT };
    if port != wanted {
        return Err("egress.port_not_allowed");
    }
    Ok(Destination { host, port, path })
}

/// A host name in the form compared with the manifest: lowercase, without a trailing dot. An address
/// written as a name, in any of the forms a resolver reads as numbers, is refused as one.
pub fn host_name(text: &str) -> Result<String, &'static str> {
    let host = text.strip_suffix('.').unwrap_or(text).to_ascii_lowercase();
    if host.parse::<IpAddr>().is_ok() {
        return Err("egress.ip_literal");
    }
    let labels: Vec<&str> = host.split('.').collect();
    let well_formed = !host.is_empty()
        && host.len() <= 253
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if !well_formed {
        return Err("egress.bad_target");
    }
    // No top-level domain is numeric, and `0x7f000001` or `127.1` would resolve as an address.
    let last = labels.last().copied().unwrap_or_default();
    if last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x") {
        return Err("egress.ip_literal");
    }
    Ok(host)
}

/// Whether the manifest lists `host`. A listed provider host is still refused
/// ([`Egress::is_provider_host`]).
pub fn listed(manifest: &Manifest, host: &str) -> bool {
    manifest
        .egress_hosts
        .iter()
        .any(|h| host_name(h).is_ok_and(|h| h == host))
}

/// A refusal the broker writes itself, as plain text naming its code.
pub fn refused(status: u16, code: &str) -> Response {
    Response {
        status,
        content_type: "text/plain".into(),
        body: Box::new(Cursor::new(
            format!("the broker refused this request: {code}\n").into_bytes(),
        )),
        reason: ReasonCode::new(code),
    }
}

/// Answers one proxy request: refuses it, or connects and relays until either end closes or the job
/// ends. Returns the response to write when the broker answers it itself.
pub fn proxy(
    request: ProxyRequest,
    reader: &mut BufReader<&UnixStream>,
    client: &UnixStream,
    manifest: &Manifest,
    egress: &Egress,
    reports: &Reports,
    ended: &Arc<AtomicBool>,
) -> io::Result<Option<Response>> {
    let to = match destination(&request.method, &request.target) {
        Ok(to) => to,
        Err(code) => {
            reports.deny(code, inputs_digest(&request.target), ended);
            return Ok(Some(refused(403, code)));
        }
    };
    if to.path.is_some() && !well_formed(&request, to.path.as_deref().unwrap_or_default()) {
        return Ok(Some(refused(400, "egress.bad_request")));
    }
    let inputs = inputs_digest(&format!("{}:{}", to.host, to.port));
    // Listed or not, a provider's host is never reached through the proxy (ADR-0016, ADR-0051).
    if egress.is_provider_host(&to.host) {
        reports.deny("egress.provider_host", inputs, ended);
        return Ok(Some(refused(403, "egress.provider_host")));
    }
    if !listed(manifest, &to.host) {
        reports.deny("egress.host_not_allowed", inputs, ended);
        return Ok(Some(refused(403, "egress.host_not_allowed")));
    }
    let addresses = match egress.resolver.resolve(&to.host, to.port) {
        Ok(addresses) => addresses,
        Err(_) => return Ok(Some(refused(502, "egress.unresolved"))),
    };
    if addresses.is_empty() {
        reports.deny("egress.not_public", inputs, ended);
        return Ok(Some(refused(403, "egress.not_public")));
    }
    let Some(upstream) = addresses
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, CONNECT_TIMEOUT).ok())
    else {
        return Ok(Some(refused(502, "egress.unreachable")));
    };
    match to.path {
        None => {
            // Bytes the harness sent after its CONNECT, such as its TLS hello, go first.
            let early = reader.buffer().to_vec();
            let mut client_out = client;
            client_out.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")?;
            upstream.set_write_timeout(Some(POLL))?;
            if send(&mut &upstream, &early, ended) {
                tunnel(client, &upstream, ended)?;
            }
        }
        Some(path) => {
            let body = match request_body(&request.headers, reader) {
                Ok(body) => body,
                Err((status, code)) => return Ok(Some(refused(status, code))),
            };
            let mut head = format!(
                "{} {path} HTTP/1.1\r\nhost: {}\r\n",
                request.method, to.host
            );
            for (name, value) in &request.headers {
                let hop = matches!(
                    name.as_str(),
                    "host"
                        | "connection"
                        | "proxy-connection"
                        | "proxy-authorization"
                        | "keep-alive"
                        | "te"
                        | "upgrade"
                        | "content-length"
                );
                if !hop {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
            }
            head.push_str(&format!(
                "content-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            ));
            upstream.set_write_timeout(Some(POLL))?;
            upstream.set_read_timeout(Some(POLL))?;
            if !send(&mut &upstream, head.as_bytes(), ended) || !send(&mut &upstream, &body, ended)
            {
                let _ = upstream.shutdown(Shutdown::Both);
                return Ok(None);
            }
            pump(&mut &upstream, &mut &*client, ended);
            let _ = client.shutdown(Shutdown::Both);
        }
    }
    let _ = upstream.shutdown(Shutdown::Both);
    Ok(None)
}

/// Whether a plain-HTTP request can be sent on as it is: a method of capital letters, a path with
/// no space or control byte, header names that are tokens and values with no control byte but a tab.
/// Anything else could be read by the host as a different request.
fn well_formed(request: &ProxyRequest, path: &str) -> bool {
    let token = |t: &str| {
        !t.is_empty()
            && t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    };
    (1..=16).contains(&request.method.len())
        && request.method.bytes().all(|b| b.is_ascii_uppercase())
        && path.bytes().all(|b| b > 0x20 && b != 0x7f)
        && request.headers.iter().all(|(name, value)| {
            token(name)
                && value
                    .bytes()
                    .all(|b| b == b'\t' || (b >= 0x20 && b != 0x7f))
        })
}

fn request_body<R: Read>(
    headers: &[(String, String)],
    reader: &mut R,
) -> Result<Vec<u8>, (u16, &'static str)> {
    let get = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v);
    if get("transfer-encoding").is_some() {
        return Err((411, "egress.chunked_request"));
    }
    let length = match get("content-length") {
        None => 0,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| (400, "egress.bad_request"))?,
    };
    if length > crate::model::MAX_BODY {
        return Err((413, "egress.too_large"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| (400, "egress.bad_request"))?;
    Ok(body)
}

/// Relays both ways until both ends have closed, either has been idle too long, or the job ends.
fn tunnel(client: &UnixStream, upstream: &TcpStream, ended: &Arc<AtomicBool>) -> io::Result<()> {
    client.set_read_timeout(Some(POLL))?;
    client.set_write_timeout(Some(POLL))?;
    upstream.set_read_timeout(Some(POLL))?;
    let (up_in, client_out) = (upstream.try_clone()?, client.try_clone()?);
    let down_ended = Arc::clone(ended);
    let down = std::thread::Builder::new().spawn(move || {
        pump(&mut &up_in, &mut &client_out, &down_ended);
        let _ = client_out.shutdown(Shutdown::Write);
    })?;
    pump(&mut &*client, &mut &*upstream, ended);
    let _ = upstream.shutdown(Shutdown::Write);
    let _ = down.join();
    let _ = client.shutdown(Shutdown::Both);
    Ok(())
}

/// Writes all of `bytes` to `to`, which has a write timeout of [`POLL`], waiting while it is full until
/// [`IDLE`] passes without progress or the job ends. A peer that stops reading cannot hold the relay.
fn send(to: &mut impl Write, mut bytes: &[u8], ended: &AtomicBool) -> bool {
    let mut idle = Duration::ZERO;
    while !bytes.is_empty() {
        if ended.load(Ordering::SeqCst) {
            return false;
        }
        match to.write(bytes) {
            Ok(0) => return false,
            Ok(n) => {
                bytes = &bytes[n..];
                idle = Duration::ZERO;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                idle += POLL;
                if idle >= IDLE {
                    return false;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
    to.flush().is_ok()
}

/// Copies from `from` to `to` until the end, an error, [`IDLE`] without a byte, or the job ending.
fn pump(from: &mut impl Read, to: &mut impl Write, ended: &AtomicBool) {
    let mut buffer = [0u8; 16 * 1024];
    let mut idle = Duration::ZERO;
    while !ended.load(Ordering::SeqCst) {
        match from.read(&mut buffer) {
            Ok(0) => return,
            Ok(n) => {
                idle = Duration::ZERO;
                if !send(to, &buffer[..n], ended) {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                idle += POLL;
                if idle >= IDLE {
                    return;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    #[test]
    fn a_base_gives_its_host_name() {
        for (base, host) in [
            ("https://api.anthropic.com", Some("api.anthropic.com")),
            ("https://LLM.Corp.Example:8443/", Some("llm.corp.example")),
            ("https://gw.example./v1", Some("gw.example")),
            ("http://localhost:9", Some("localhost")),
            ("http://127.0.0.1:9", None),
            ("https://[::1]:443", None),
            ("gw.example", None),
        ] {
            assert_eq!(base_host(base).as_deref(), host, "{base}");
        }
    }

    #[test]
    fn a_provider_host_and_every_name_under_it_are_refused() {
        let egress = Egress::public().refusing_bases(["https://llm.corp.example:8443"]);
        for host in [
            "api.anthropic.com",
            "api.openai.com",
            "eu.api.openai.com",
            "a.b.api.anthropic.com",
            "llm.corp.example",
            "x.llm.corp.example",
        ] {
            assert!(egress.is_provider_host(host), "{host}");
        }
        for host in [
            "anthropic.com",
            "docs.anthropic.com",
            "notapi.openai.com",
            "api.openai.com.evil.example",
            "corp.example",
            "xllm.corp.example",
        ] {
            assert!(!egress.is_provider_host(host), "{host}");
        }
        assert!(!Egress::public().is_provider_host("llm.corp.example"));
    }

    /// A host that accepts and never reads cannot hold a write: it gives up when the job ends.
    #[test]
    fn a_write_to_a_host_that_stops_reading_ends_with_the_job()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let stream = TcpStream::connect(listener.local_addr()?)?;
        let (_held, _) = listener.accept()?;
        stream.set_write_timeout(Some(POLL))?;
        let ended = Arc::new(AtomicBool::new(false));
        let ending = Arc::clone(&ended);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            ending.store(true, Ordering::SeqCst);
        });
        let began = Instant::now();
        // Far more than the socket buffers hold.
        let sent = send(&mut &stream, &vec![0u8; 64 * 1024 * 1024], &ended);
        assert!(!sent);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the write outlived its job"
        );
        Ok(())
    }
}
