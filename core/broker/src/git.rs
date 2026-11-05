//! The git proxy (ADR-0046, S3.3).
//!
//! A job clones, fetches and pushes over git's smart HTTP protocol to `/git/{owner}/{repo}.git/..` on
//! its socket. The broker passes a request on to the forge only for the manifest's repository, only for
//! a service the manifest's actions allow (fetching needs `read`, pushing needs `push`), and adds the
//! job's forge token itself, so nothing in the sandbox is a credential. Before a push is sent, the broker
//! reads the ref updates it asks for and refuses the whole push if any is outside the manifest's branch
//! prefix, is not a branch, or deletes one. A refusal is a 403 to the harness and the manifest gate's
//! `deny` to the engine, carrying the SHA-256 of the repository or ref asked for, never the name.
//!
//! A push that passes those rules goes through the content gate (ADR-0054): its push options, and
//! every object in its pack once inflated (`pack::check`). So that every delta's base is in the pack,
//! the broker adds `no-thin` to the forge's push advertisement.

use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use hyalos_types::{ForgeToken, Manifest};

use crate::content;
use crate::egress::{Gates, ProxyRequest, inputs_digest, refused};
use crate::model::{HttpTransport, Response, Upstream, dechunk};
use crate::pack::{self, Finding};
use hyalos_types::Digest;
use sha2::{Digest as _, Sha256};

/// Where git requests arrive on a job's socket.
pub const PREFIX: &str = "/git/";
/// The largest request body the broker reads for git, such as a push's pack. A larger push is refused.
pub const MAX_GIT_BODY: usize = 64 * 1024 * 1024;
/// Reports one job's git refusals may make. After this many, one more says the rest were not made.
pub const TOO_MANY: &str = "git.too_many_refusals";
/// Held while a push's pack is checked (ADR-0054).
static CHECKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// The forge's git host when none is given: github.com.
pub const GITHUB: &str = "https://github.com";

/// The forge's git host.
#[derive(Debug, Clone)]
pub struct Git {
    transport: Result<HttpTransport, &'static str>,
}

impl Default for Git {
    fn default() -> Self {
        Self::github()
    }
}

impl Git {
    /// github.com, over TLS to the compiled-in roots (ADR-0039).
    pub fn github() -> Self {
        Self::with_base(GITHUB)
    }

    /// Another git host: the one the engine names in `Hello` (ADR-0050), or a test's on loopback.
    pub fn with_base(base: &str) -> Self {
        Self {
            transport: HttpTransport::new(base).map_err(|e| e.0),
        }
    }
}

/// Which side of git's protocol a request is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// Clone and fetch.
    UploadPack,
    /// Push.
    ReceivePack,
}

impl Service {
    fn name(self) -> &'static str {
        match self {
            Service::UploadPack => "git-upload-pack",
            Service::ReceivePack => "git-receive-pack",
        }
    }

    /// The manifest action that allows it.
    fn action(self) -> &'static str {
        match self {
            Service::UploadPack => "read",
            Service::ReceivePack => "push",
        }
    }
}

/// A git request, as read from its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// `owner/name`, without `.git`, as written.
    pub repo: String,
    pub service: Service,
    /// The ref advertisement (`GET info/refs`), rather than the call itself (`POST`).
    pub advertisement: bool,
}

/// Reads a git request's method and target. `None` is not a request the broker serves.
pub fn route(method: &str, target: &str) -> Option<Route> {
    let rest = target.strip_prefix(PREFIX)?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut parts = path.splitn(3, '/');
    let (owner, name, call) = (parts.next()?, parts.next()?, parts.next()?);
    let name = name.strip_suffix(".git").unwrap_or(name);
    let part = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if !part(owner) || !part(name) {
        return None;
    }
    let service = |s: &str| match s {
        "git-upload-pack" => Some(Service::UploadPack),
        "git-receive-pack" => Some(Service::ReceivePack),
        _ => None,
    };
    let (service, advertisement) = match (method, call) {
        ("GET", "info/refs") => (service(query.strip_prefix("service=")?)?, true),
        ("POST", call) if query.is_empty() => (service(call)?, false),
        _ => return None,
    };
    Some(Route {
        repo: format!("{owner}/{name}"),
        service,
        advertisement,
    })
}

/// One ref update a push asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub old: String,
    pub new: String,
    pub refname: String,
}

impl Command {
    fn deletes(&self) -> bool {
        self.new.bytes().all(|b| b == b'0')
    }
}

/// A push's body, as read: its ref updates, its push options and its pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Push<'a> {
    pub commands: Vec<Command>,
    /// Sent only when the push asked for `push-options`; the forge reads them.
    pub options: Vec<&'a str>,
    pub pack: &'a [u8],
}

/// Reads the ref updates at the start of a push's body: pkt-lines up to the first flush. `Err` is the
/// reason the push is refused.
pub fn commands(body: &[u8]) -> Result<Vec<Command>, &'static str> {
    read_push(body).map(|push| push.commands)
}

/// One pkt-line at `at`: `Some` with its contents, or `None` for a flush. `at` moves past it.
fn pkt_line<'a>(body: &'a [u8], at: &mut usize) -> Result<Option<&'a [u8]>, &'static str> {
    let bad = "git.bad_request";
    let size = body.get(*at..*at + 4).ok_or(bad)?;
    let size = std::str::from_utf8(size).map_err(|_| bad)?;
    let size = usize::from_str_radix(size, 16).map_err(|_| bad)?;
    if size == 0 {
        *at += 4;
        return Ok(None);
    }
    if size < 4 {
        return Err(bad);
    }
    let line = body.get(*at + 4..*at + size).ok_or(bad)?;
    *at += size;
    Ok(Some(line))
}

/// Reads a push's body: its ref updates up to the first flush, then its push options up to the next
/// if it asked for them, then its pack.
pub fn read_push(body: &[u8]) -> Result<Push<'_>, &'static str> {
    let bad = "git.bad_request";
    let mut at = 0;
    let mut out = Vec::new();
    let mut options = false;
    while let Some(line) = pkt_line(body, &mut at)? {
        // Capabilities follow the first command, after a NUL.
        let mut parts = line.splitn(2, |b| *b == 0);
        let line = parts.next().unwrap_or_default();
        if let Some(capabilities) = parts.next() {
            options |= capabilities
                .split(|b| b.is_ascii_whitespace())
                .any(|c| c == b"push-options");
        }
        let line = std::str::from_utf8(line).map_err(|_| bad)?;
        let line = line.strip_suffix('\n').unwrap_or(line);
        if line.starts_with("push-cert") {
            return Err("git.push_cert");
        }
        if let Some(oid) = line.strip_prefix("shallow ") {
            if !object_id(oid) {
                return Err(bad);
            }
            continue;
        }
        let mut words = line.split(' ');
        let (Some(old), Some(new), Some(refname), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return Err(bad);
        };
        if !object_id(old) || !object_id(new) || old.len() != new.len() {
            return Err(bad);
        }
        out.push(Command {
            old: old.into(),
            new: new.into(),
            refname: refname.into(),
        });
    }
    let mut pushed = Vec::new();
    if options {
        while let Some(line) = pkt_line(body, &mut at)? {
            let line = std::str::from_utf8(line).map_err(|_| bad)?;
            pushed.push(line.strip_suffix('\n').unwrap_or(line));
        }
    }
    Ok(Push {
        commands: out,
        options: pushed,
        pack: &body[at..],
    })
}

/// Adds `no-thin` to a push's ref advertisement, so the job's git sends a pack whose deltas all have
/// their base in it (ADR-0054). `None` is an advertisement without a capability list.
pub fn no_thin(advertisement: &[u8]) -> Option<Vec<u8>> {
    let mut at = 0;
    while at < advertisement.len() {
        let start = at;
        let line = match pkt_line(advertisement, &mut at).ok()? {
            None => continue,
            Some(line) => line,
        };
        let Some(nul) = line.iter().position(|b| *b == 0) else {
            continue;
        };
        let capabilities = &line[nul + 1..];
        let (capabilities, newline) = match capabilities.strip_suffix(b"\n") {
            Some(c) => (c, &b"\n"[..]),
            None => (capabilities, &b""[..]),
        };
        if capabilities.split(|b| *b == b' ').any(|c| c == b"no-thin") {
            return Some(advertisement.to_vec());
        }
        let mut changed = line[..=nul].to_vec();
        changed.extend_from_slice(capabilities);
        if !capabilities.is_empty() {
            changed.push(b' ');
        }
        changed.extend_from_slice(b"no-thin");
        changed.extend_from_slice(newline);
        let mut out = advertisement[..start].to_vec();
        out.extend_from_slice(format!("{:04x}", changed.len() + 4).as_bytes());
        out.extend_from_slice(&changed);
        out.extend_from_slice(&advertisement[at..]);
        return Some(out);
    }
    None
}

/// The content gate on a push that the manifest's rules allow: `Err` is the answer, the reason and
/// what the report's digest is of.
fn content_gate(push: &Push<'_>, secrets: &[&str]) -> Result<(), (bool, &'static str, Digest)> {
    for option in &push.options {
        if let Err(code) = content::check(option, secrets) {
            return Err((false, code, inputs_digest(option)));
        }
    }
    // Object ids are 20 bytes, or 32 in a SHA-256 repository: the commands say which.
    let id_len = push.commands.first().map_or(20, |c| c.new.len() / 2);
    match pack::check(push.pack, id_len, secrets) {
        Ok(()) => Ok(()),
        Err(Finding::Refused(code, digest)) => Err((false, code, Digest::from_bytes(digest))),
        Err(Finding::Unchecked(code)) => Err((
            true,
            code,
            Digest::from_bytes(Sha256::digest(push.pack).into()),
        )),
    }
}

fn object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether a push may update `refname`: a branch, well formed, inside the manifest's branch prefix. A
/// prefix ending in `/` covers the branches under it; any other covers itself and the branches under
/// it, so `hyalos/3` does not cover `hyalos/30`. An empty prefix covers nothing.
pub fn ref_allowed(manifest: &Manifest, refname: &str) -> bool {
    let Some(branch) = refname.strip_prefix("refs/heads/") else {
        return false;
    };
    let prefix = manifest.branch_prefix.as_str();
    let inside = if prefix.is_empty() {
        false
    } else if prefix.ends_with('/') {
        branch.len() > prefix.len() && branch.starts_with(prefix)
    } else {
        branch == prefix
            || branch
                .strip_prefix(prefix)
                .is_some_and(|r| r.starts_with('/'))
    };
    inside && well_formed_ref(branch)
}

/// The rules of `git check-ref-format`, kept to printable ASCII.
pub(crate) fn well_formed_ref(name: &str) -> bool {
    !name.is_empty()
        && name != "@"
        && !name.contains("..")
        && !name.contains("@{")
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|b| b > 0x20 && b < 0x7f && !b"~^:?*[\\".contains(&b))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

/// Answers one git request for the job whose admitted manifest is `manifest` and whose forge token is
/// `token`. `reader` holds the rest of the request; a `100 Continue` is written to `client` if asked.
/// `secrets` are the provider keys the content gate looks for, with the token. Refusals go to
/// `gates`: the manifest gate's, or the content gate's for what a push carries.
#[allow(clippy::too_many_arguments)]
pub(crate) fn answer<R: BufRead, W: Write>(
    request: &ProxyRequest,
    reader: &mut R,
    client: &mut W,
    manifest: &Manifest,
    token: Option<&ForgeToken>,
    git: &Git,
    secrets: &[&str],
    gates: Gates<'_>,
    ended: &AtomicBool,
) -> Response {
    let reports = gates.manifest;
    let deny = |code: &'static str, inputs: &str| {
        reports.deny(code, inputs_digest(inputs), ended);
        refused(403, code)
    };
    let Some(route) = route(&request.method, &request.target) else {
        return refused(404, "git.route_refused");
    };
    if !route.repo.eq_ignore_ascii_case(&manifest.repo) {
        return deny("git.repo_not_allowed", &route.repo.to_ascii_lowercase());
    }
    if !manifest.actions.contains(route.service.action()) {
        return deny("git.action_not_allowed", route.service.name());
    }
    let Some(token) = token else {
        return refused(403, "git.no_token");
    };
    let transport = match &git.transport {
        Ok(transport) => transport,
        Err(code) => return refused(502, code),
    };
    let header = |name: &str| {
        request
            .headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    let body = if route.advertisement {
        Vec::new()
    } else {
        // A push's pack is not compressed again; only a fetch's request may be.
        if route.service == Service::ReceivePack && header("content-encoding").is_some() {
            return refused(415, "git.bad_request");
        }
        let body = match read_body(header, reader, client, MAX_GIT_BODY) {
            Ok(body) => body,
            Err((status, code)) => return refused(status, code),
        };
        if route.service == Service::ReceivePack {
            let push = match read_push(&body) {
                Ok(push) => push,
                Err(code) => return refused(400, code),
            };
            // Every refused ref is reported; one is enough to refuse the push.
            let mut refusal = None;
            for command in &push.commands {
                let code = if !ref_allowed(manifest, &command.refname) {
                    "git.ref_not_allowed"
                } else if command.deletes() {
                    "git.delete_refused"
                } else {
                    continue;
                };
                reports.deny(code, inputs_digest(&command.refname), ended);
                refusal.get_or_insert(code);
            }
            if let Some(code) = refusal {
                return refused(403, code);
            }
            let mut checked = secrets.to_vec();
            checked.push(token.expose());
            // One push is inflated at a time, so the broker holds at most one `MAX_INFLATED`.
            let gated = {
                let _one = CHECKING.lock().unwrap_or_else(|p| p.into_inner());
                content_gate(&push, &checked)
            };
            if let Err((unknown, code, digest)) = gated {
                if unknown {
                    gates.content.unknown(code, digest, ended);
                } else {
                    gates.content.deny(code, digest, ended);
                }
                return refused(403, code);
            }
        }
        body
    };
    if ended.load(Ordering::SeqCst) {
        return refused(403, "admission.ended");
    }
    let mut headers = vec![(
        "authorization".to_string(),
        format!(
            "Basic {}",
            base64(format!("x-access-token:{}", token.expose()).as_bytes())
        ),
    )];
    let mut pass = vec!["accept", "content-type", "git-protocol", "user-agent"];
    if route.service == Service::UploadPack {
        pass.push("content-encoding");
    }
    for name in pass {
        if let Some(value) = header(name) {
            headers.push((name.to_string(), value.to_string()));
        }
    }
    // The manifest's repository, not the request's spelling of it.
    let call = if route.advertisement {
        format!("info/refs?service={}", route.service.name())
    } else {
        route.service.name().to_string()
    };
    let upstream = Upstream {
        target: format!("/{}.git/{call}", manifest.repo),
        headers,
        body,
    };
    let method = if route.advertisement { "GET" } else { "POST" };
    match transport.request(method, &upstream) {
        Err(e) => refused(502, e.0),
        // The forge's challenge is not passed on: the harness holds no credential to answer with.
        Ok(r) if r.status == 401 || r.status == 403 => refused(403, "git.upstream_refused"),
        Ok(r) if r.status == 404 => refused(404, "git.upstream_not_found"),
        Ok(mut r) if route.advertisement && route.service == Service::ReceivePack => {
            let mut advertisement = Vec::new();
            let read = (&mut r.body)
                .take(MAX_GIT_BODY as u64 + 1)
                .read_to_end(&mut advertisement);
            let changed = read
                .ok()
                .filter(|_| r.status == 200 && advertisement.len() <= MAX_GIT_BODY)
                .and_then(|_| no_thin(&advertisement));
            match changed {
                Some(changed) => Response {
                    status: 200,
                    content_type: "application/x-git-receive-pack-advertisement".into(),
                    body: Box::new(std::io::Cursor::new(changed)),
                    reason: None,
                },
                None => refused(502, "git.bad_advertisement"),
            }
        }
        Ok(r) => Response {
            status: r.status,
            content_type: r
                .headers
                .iter()
                .find(|(n, _)| n == "content-type")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "application/octet-stream".into()),
            body: r.body,
            reason: None,
        },
    }
}

/// Reads a git request's body, by `content-length` or chunked as git sends a large one, after
/// answering `100 Continue` if the harness waits for it.
pub(crate) fn read_body<'h, R: BufRead, W: Write>(
    header: impl Fn(&str) -> Option<&'h str>,
    reader: &mut R,
    client: &mut W,
    max: usize,
) -> Result<Vec<u8>, (u16, &'static str)> {
    let bad = (400, "git.bad_request");
    let chunked = match (header("transfer-encoding"), header("content-length")) {
        (Some(te), None) if te.eq_ignore_ascii_case("chunked") => true,
        (None, _) => false,
        _ => return Err(bad),
    };
    if header("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
        client
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .and_then(|()| client.flush())
            .map_err(|_| bad)?;
    }
    let too_large = (413, "git.too_large");
    let mut body = Vec::new();
    if chunked {
        dechunk(reader)
            .take(max as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|_| bad)?;
        if body.len() > max {
            return Err(too_large);
        }
    } else {
        let length = header("content-length")
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| bad)?
            .unwrap_or(0);
        if length > max {
            return Err(too_large);
        }
        body.resize(length, 0);
        reader.read_exact(&mut body).map_err(|_| bad)?;
    }
    Ok(body)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyalos_types::{Budget, JobId, MANIFEST_VERSION, Role, Timestamp};

    fn manifest(prefix: &str) -> Manifest {
        Manifest {
            version: MANIFEST_VERSION,
            job: JobId::from_seq(1),
            role: Role::Actor,
            tainted: true,
            repo: "octo/repo".into(),
            branch_prefix: prefix.into(),
            pr_base: None,
            actions: Default::default(),
            egress_hosts: Default::default(),
            tools: Default::default(),
            data_flows: Default::default(),
            budget: Budget { tokens: 1 },
            deadline: Timestamp::from_millis(1),
            approval: None,
        }
    }

    fn pkt(line: &str) -> String {
        format!("{:04x}{line}", line.len() + 4)
    }

    #[test]
    fn routes_are_the_smart_http_calls_and_nothing_else() {
        let r = route(
            "GET",
            "/git/octo/repo.git/info/refs?service=git-upload-pack",
        )
        .unwrap();
        assert_eq!(
            r,
            Route {
                repo: "octo/repo".into(),
                service: Service::UploadPack,
                advertisement: true
            }
        );
        let r = route("POST", "/git/Octo/repo/git-receive-pack").unwrap();
        assert_eq!(
            (r.repo.as_str(), r.service, r.advertisement),
            ("Octo/repo", Service::ReceivePack, false)
        );
        for (method, target) in [
            ("GET", "/git/octo/repo.git/info/refs"),
            (
                "GET",
                "/git/octo/repo.git/info/refs?service=git-upload-archive",
            ),
            (
                "POST",
                "/git/octo/repo.git/info/refs?service=git-upload-pack",
            ),
            ("GET", "/git/octo/repo.git/git-upload-pack"),
            ("POST", "/git/octo/repo.git/git-upload-pack?x=1"),
            ("POST", "/git/../repo.git/git-upload-pack"),
            ("POST", "/git/octo/re po/git-upload-pack"),
            ("POST", "/git/octo/git-upload-pack"),
            ("GET", "/git/octo/repo.git/HEAD"),
            ("GET", "/v1/messages"),
        ] {
            assert_eq!(route(method, target), None, "{method} {target}");
        }
    }

    #[test]
    fn a_push_is_read_up_to_its_first_flush() {
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        let body = format!(
            "{}{}{}0000PACK..",
            pkt(&format!("shallow {a}\n")),
            pkt(&format!(
                "{a} {b} refs/heads/hyalos/x\0report-status side-band-64k\n"
            )),
            pkt(&format!("{a} {} refs/heads/hyalos/y\n", "0".repeat(40))),
        );
        let read = commands(body.as_bytes()).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].refname, "refs/heads/hyalos/x");
        assert!(!read[0].deletes() && read[1].deletes());
        assert_eq!(commands(b"0000").unwrap(), vec![]);
        for bad in [
            "".to_string(),
            "00".to_string(),
            "zzzz".to_string(),
            "0001".to_string(),
            pkt("short line\n"),
            pkt(&format!("{a} {b} refs/heads/x")),
            pkt(&format!("{a} {b} refs/heads/x extra\n")) + "0000",
            pkt(&format!("{a} {} refs/heads/x\n", "b".repeat(64))) + "0000",
            pkt(&format!("{} {b} refs/heads/x\n", "A".repeat(40))) + "0000",
        ] {
            assert_eq!(commands(bad.as_bytes()), Err("git.bad_request"), "{bad:?}");
        }
        assert_eq!(
            commands(pkt("push-cert\0report-status\n").as_bytes()),
            Err("git.push_cert")
        );
    }

    #[test]
    fn push_options_and_the_pack_follow_the_commands() {
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        let body = format!(
            "{}0000{}{}0000PACK..",
            pkt(&format!(
                "{a} {b} refs/heads/hyalos/x\0report-status push-options\n"
            )),
            pkt("ci.skip\n"),
            pkt("note=hello"),
        );
        let push = read_push(body.as_bytes()).unwrap();
        assert_eq!(push.commands.len(), 1);
        assert_eq!(push.options, vec!["ci.skip", "note=hello"]);
        assert_eq!(push.pack, b"PACK..");
        // Without the capability, what follows the flush is the pack.
        let body = format!(
            "{}0000PACK",
            pkt(&format!("{a} {b} refs/heads/hyalos/x\0report-status\n"))
        );
        let push = read_push(body.as_bytes()).unwrap();
        assert!(push.options.is_empty());
        assert_eq!(push.pack, b"PACK");
        let unended = format!(
            "{}0000{}",
            pkt(&format!("{a} {b} refs/heads/x\0push-options\n")),
            pkt("x")
        );
        assert_eq!(read_push(unended.as_bytes()), Err("git.bad_request"));
    }

    #[test]
    fn a_push_advertisement_gains_no_thin() {
        let a = "a".repeat(40);
        let advert = |first: &str| {
            format!(
                "{}0000{}{}0000",
                pkt("# service=git-receive-pack\n"),
                pkt(first),
                pkt(&format!("{a} refs/heads/dev\n")),
            )
        };
        let changed =
            no_thin(advert(&format!("{a} refs/heads/main\0report-status ofs-delta\n")).as_bytes());
        assert_eq!(
            changed.as_deref(),
            Some(
                advert(&format!(
                    "{a} refs/heads/main\0report-status ofs-delta no-thin\n"
                ))
                .as_bytes()
            )
        );
        // An empty repository advertises its capabilities on a placeholder.
        let zero = "0".repeat(40);
        let changed = no_thin(advert(&format!("{zero} capabilities^{{}}\0ofs-delta")).as_bytes());
        assert_eq!(
            changed.as_deref(),
            Some(advert(&format!("{zero} capabilities^{{}}\0ofs-delta no-thin")).as_bytes())
        );
        let already = advert(&format!("{a} refs/heads/main\0no-thin\n"));
        assert_eq!(
            no_thin(already.as_bytes()).as_deref(),
            Some(already.as_bytes())
        );
        assert_eq!(
            no_thin(format!("{}0000", pkt("# service=git-receive-pack\n")).as_bytes()),
            None
        );
        assert_eq!(no_thin(b"zzzz"), None);
    }

    #[test]
    fn only_branches_inside_the_prefix_may_be_pushed() {
        let m = manifest("hyalos/");
        for ok in ["refs/heads/hyalos/x", "refs/heads/hyalos/job-3/fix"] {
            assert!(ref_allowed(&m, ok), "{ok}");
        }
        for no in [
            "refs/heads/hyalos/",
            "refs/heads/hyalos",
            "refs/heads/main",
            "refs/heads/hyalos-x",
            "refs/tags/hyalos/v1",
            "refs/heads/hyalos/../main",
            "refs/heads/hyalos/x.lock",
            "refs/heads/hyalos/.x",
            "refs/heads/hyalos//x",
            "refs/heads/hyalos/x@{1}",
            "refs/heads/hyalos/x y",
            "refs/heads/hyalos/x~1",
            "hyalos/x",
            "refs/pull/1/head",
        ] {
            assert!(!ref_allowed(&m, no), "{no}");
        }
        let m = manifest("hyalos/3");
        assert!(ref_allowed(&m, "refs/heads/hyalos/3"));
        assert!(ref_allowed(&m, "refs/heads/hyalos/3/x"));
        assert!(!ref_allowed(&m, "refs/heads/hyalos/30"));
        assert!(!ref_allowed(&manifest(""), "refs/heads/main"));
    }

    #[test]
    fn a_body_is_read_chunked_or_by_length_after_any_continue() {
        let headers = |list: &'static [(&'static str, &'static str)]| {
            move |n: &str| list.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
        };
        let mut client = Vec::new();
        let mut input = std::io::Cursor::new(b"5\r\nhello\r\n0\r\n\r\n".to_vec());
        let body = read_body(
            headers(&[("transfer-encoding", "chunked"), ("expect", "100-continue")]),
            &mut input,
            &mut client,
            MAX_GIT_BODY,
        );
        assert_eq!(body.as_deref(), Ok(&b"hello"[..]));
        assert_eq!(client, b"HTTP/1.1 100 Continue\r\n\r\n");

        let mut client = Vec::new();
        let mut input = std::io::Cursor::new(b"abc".to_vec());
        let body = read_body(
            headers(&[("content-length", "3")]),
            &mut input,
            &mut client,
            MAX_GIT_BODY,
        );
        assert_eq!(body.as_deref(), Ok(&b"abc"[..]));
        assert!(client.is_empty(), "no continue was asked for");

        for bad in [
            &[("transfer-encoding", "chunked"), ("content-length", "3")][..],
            &[("transfer-encoding", "gzip")][..],
            &[("content-length", "x")][..],
        ] {
            let list: &'static [(&str, &str)] = Box::leak(bad.to_vec().into_boxed_slice());
            let body = read_body(
                headers(list),
                &mut std::io::Cursor::new(Vec::new()),
                &mut Vec::new(),
                MAX_GIT_BODY,
            );
            assert_eq!(body, Err((400, "git.bad_request")), "{bad:?}");
        }
        let body = read_body(
            headers(&[("content-length", "67108865")]),
            &mut std::io::Cursor::new(Vec::new()),
            &mut Vec::new(),
            MAX_GIT_BODY,
        );
        assert_eq!(body, Err((413, "git.too_large")));
    }

    #[test]
    fn basic_credentials_are_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(
            base64(b"x-access-token:ghs_x"),
            "eC1hY2Nlc3MtdG9rZW46Z2hzX3g="
        );
    }
}
