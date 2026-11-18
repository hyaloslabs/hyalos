//! The broker's side facing a job's sandbox (ADR-0016, ADR-0038).
//!
//! When a job is admitted the broker listens on a Unix socket made for that job alone, at the path
//! the engine chose. The engine mounts it in the job's sandbox, where the harness's forwarder
//! connects to it. Which socket a call arrives on is which job it is for, so nothing in the sandbox
//! is a credential. Each connection carries one HTTP/1.1 model call, which goes to the relay for its
//! protocol under the job's admitted manifest; the provider key is added there and never sent back.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use hyalos_types::wire::ToEngine;
use hyalos_types::{Clock, ForgeToken, Gate, Manifest, ReasonCode, flow};
use std::sync::mpsc::Sender;

use crate::egress::{self, Egress, Gates, ProxyRequest, Reports};
use crate::forge::Forge;
use crate::git::{self, Git};
use crate::handles::{self, Held};
use crate::model::{
    Adapter, Anthropic, ApiKey, HttpTransport, MAX_BODY, Meter, OpenAi, Relay, Request, Response,
    Sleep, refusal,
};
use crate::pulls;
use crate::submit::{self, Submission, Submitted};

pub use hyalos_types::wire::MAX_SOCKET_PATH;
/// Calls one job may have open at once. Another connection is closed unanswered.
const MAX_CONNECTIONS: usize = 16;
const MAX_HEAD: u64 = 16 * 1024;
const MAX_HEADERS: usize = 100;
/// How long the harness may take to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How often the accept loop looks for a connection or a stop.
const ACCEPT_POLL: Duration = Duration::from_millis(20);
/// How long a write to the harness may block while a response streams.
const WRITE_TIMEOUT: Duration = Duration::from_secs(300);

/// One provider's key and where to reach it.
#[derive(Debug, Clone)]
pub struct Provider {
    pub key: ApiKey,
    pub base: String,
}

/// The providers the broker may call, read from the operator's key file.
#[derive(Debug, Clone, Default)]
pub struct Providers {
    pub anthropic: Option<Provider>,
    pub openai: Option<Provider>,
}

/// Why a key file was refused. It names a line or a provider, never what the line held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// Not `name = value`, or a name this build does not know.
    Line(usize),
    /// A name given twice.
    Repeated(usize),
    /// A key that is not printable ASCII without spaces.
    Key(&'static str),
    /// A base given for a provider with no key.
    BaseWithoutKey(&'static str),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Line(n) => write!(
                f,
                "line {n} is not one of anthropic.key, anthropic.base, openai.key or openai.base = value"
            ),
            ProviderError::Repeated(n) => write!(f, "line {n} gives a name a second time"),
            ProviderError::Key(p) => write!(f, "the {p} key is not a key"),
            ProviderError::BaseWithoutKey(p) => write!(f, "{p}.base is given with no {p}.key"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl Providers {
    /// Reads `name = value` lines; blank lines and lines starting with `#` are skipped. The names are
    /// `anthropic.key`, `anthropic.base`, `openai.key` and `openai.base`. A base defaults to the
    /// provider's public API.
    pub fn parse(text: &str) -> Result<Self, ProviderError> {
        let mut values: [Option<&str>; 4] = [None; 4];
        const NAMES: [&str; 4] = [
            "anthropic.key",
            "anthropic.base",
            "openai.key",
            "openai.base",
        ];
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, value) = line
                .split_once('=')
                .map(|(n, v)| (n.trim(), v.trim()))
                .ok_or(ProviderError::Line(i + 1))?;
            let slot = NAMES
                .iter()
                .position(|n| *n == name)
                .filter(|_| !value.is_empty())
                .ok_or(ProviderError::Line(i + 1))?;
            if values[slot].replace(value).is_some() {
                return Err(ProviderError::Repeated(i + 1));
            }
        }
        let provider = |name: &'static str,
                        key: Option<&str>,
                        base: Option<&str>,
                        default: &str| match (key, base) {
            (None, None) => Ok(None),
            (None, Some(_)) => Err(ProviderError::BaseWithoutKey(name)),
            (Some(key), base) => Ok(Some(Provider {
                key: ApiKey::new(key).ok_or(ProviderError::Key(name))?,
                base: base.unwrap_or(default).to_string(),
            })),
        };
        Ok(Self {
            anthropic: provider(
                "anthropic",
                values[0],
                values[1],
                "https://api.anthropic.com",
            )?,
            openai: provider("openai", values[2], values[3], "https://api.openai.com")?,
        })
    }
}

impl Providers {
    /// The bases the providers are reached at, whose hosts the egress gate never reaches (ADR-0051).
    pub fn bases(&self) -> impl Iterator<Item = &str> {
        [&self.anthropic, &self.openai]
            .into_iter()
            .flatten()
            .map(|p| p.base.as_str())
    }
}

/// The relays for each protocol, and the meter they share. A protocol with no usable provider holds
/// the reason instead, and its calls are refused with it.
pub struct Models {
    anthropic: Result<Relay, &'static str>,
    openai: Result<Relay, &'static str>,
    meter: Arc<Meter>,
}

impl fmt::Debug for Models {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Models")
            .field("anthropic", &self.anthropic.is_ok())
            .field("openai", &self.openai.is_ok())
            .finish()
    }
}

impl Models {
    /// Relays as given, sharing `meter`. `Err` holds the reason code a call is refused with.
    pub fn new(
        anthropic: Result<Relay, &'static str>,
        openai: Result<Relay, &'static str>,
        meter: Arc<Meter>,
    ) -> Self {
        Self {
            anthropic,
            openai,
            meter,
        }
    }

    /// Relays over [`HttpTransport`] for the providers in the key file. A provider whose base this
    /// transport refuses (plain `http://` off this host) is refused with the transport's code.
    pub fn from_providers(providers: &Providers, sleep: Arc<dyn Sleep>) -> Self {
        let meter = Arc::new(Meter::default());
        let relay = |provider: &Option<Provider>, adapter: Arc<dyn Adapter>| {
            let provider = provider.as_ref().ok_or("model.no_provider")?;
            let transport = HttpTransport::new(&provider.base).map_err(|e| e.0)?;
            Ok(Relay::new(
                adapter,
                Arc::new(transport),
                provider.key.clone(),
                Arc::clone(&meter),
                Arc::clone(&sleep),
            ))
        };
        Self {
            anthropic: relay(&providers.anthropic, Arc::new(Anthropic)),
            openai: relay(&providers.openai, Arc::new(OpenAi)),
            meter: Arc::clone(&meter),
        }
    }

    pub fn meter(&self) -> &Arc<Meter> {
        &self.meter
    }

    /// The provider keys, which outbound text must never hold (ADR-0048).
    pub(crate) fn keys(&self) -> Vec<&str> {
        [&self.anthropic, &self.openai]
            .into_iter()
            .filter_map(|r| r.as_ref().ok())
            .map(|r| r.key().expose())
            .collect()
    }

    /// Answers one call for the job whose admitted manifest is `manifest`.
    pub fn answer(&self, manifest: &Manifest, request: Request, clock: &dyn Clock) -> Response {
        let path = request.target.split('?').next().unwrap_or_default();
        let (adapter, relay): (&dyn Adapter, _) = if path.starts_with("/v1/messages") {
            (&Anthropic, &self.anthropic)
        } else if path.starts_with("/v1/responses") {
            (&OpenAi, &self.openai)
        } else {
            return refusal(
                &OpenAi,
                404,
                "model.route_refused",
                "the broker serves no model call at this path",
            );
        };
        // The manifest is enforced on every call, not only at admission (ADR-0004).
        if clock.now() > manifest.deadline {
            return refusal(
                adapter,
                403,
                "admission.expired",
                "refused by the job's manifest: its deadline has passed",
            );
        }
        match relay {
            Ok(relay) => relay.handle(manifest, request),
            Err(code) => refusal(
                adapter,
                403,
                code,
                &format!("the broker has no usable key for this provider ({code})"),
            ),
        }
    }
}

/// What the broker needs to serve sandboxes: the relays, how to reach allowed hosts and the forge's git
/// host, and a clock its threads can share.
#[derive(Clone)]
pub struct Sandboxes {
    pub models: Arc<Models>,
    pub egress: Egress,
    pub git: Git,
    /// The forge's API, for the pull requests jobs open (ADR-0048).
    pub forge: Forge,
    pub clock: Arc<dyn Clock + Send + Sync>,
}

impl fmt::Debug for Sandboxes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sandboxes")
            .field("models", &self.models)
            .finish()
    }
}

/// A job's socket. Dropping it stops listening and removes the socket file.
#[derive(Debug)]
pub(crate) struct JobSocket {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    /// What the job's harness has submitted (ADR-0064).
    submitted: Submission,
    thread: Option<JoinHandle<()>>,
}

impl JobSocket {
    /// Listens at `path` for the job `manifest` is for, whose git calls carry `forge_token` and which
    /// may read the raw text of `held`, the handles its manifest names (ADR-0063). The path must be
    /// absolute, short, and not exist; its directory is the engine's, made for this job. Egress and
    /// git refusals go to `reports`.
    pub(crate) fn open(
        path: &Path,
        manifest: Manifest,
        forge_token: Option<ForgeToken>,
        held: Held,
        sandboxes: Sandboxes,
        reports: Option<Sender<ToEngine>>,
    ) -> io::Result<Self> {
        let text = path.to_string_lossy();
        if !path.is_absolute() || text.len() > MAX_SOCKET_PATH || text.contains('\0') {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let listener = UnixListener::bind(path)?;
        let ready = (|| {
            // The sandbox runs as another user (ADR-0032); the directory's name keeps others out.
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
            // Polled, so stopping never depends on the socket file still being there.
            listener.set_nonblocking(true)
        })();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let reports = Arc::new(JobReports {
            egress: Reports::new(manifest.job, reports.clone()),
            manifest: Reports::for_gate(
                manifest.job,
                Gate::Manifest,
                git::TOO_MANY,
                reports.clone(),
            ),
            content: Reports::for_gate(
                manifest.job,
                Gate::Content,
                "content.too_many_refusals",
                reports.clone(),
            ),
            role: Reports::for_gate(
                manifest.job,
                Gate::Role,
                "role.too_many_refusals",
                reports.clone(),
            ),
            dependency: Reports::for_gate(
                manifest.job,
                Gate::Dependency,
                "dependency.too_many_losses",
                reports,
            ),
        });
        let submitted = Submission::default();
        let manifest = Arc::new(Job {
            manifest,
            forge_token,
            held,
            submitted: Arc::clone(&submitted),
        });
        let thread = ready.and_then(|()| {
            std::thread::Builder::new().spawn(move || {
                let active = Arc::new(AtomicUsize::new(0));
                while !stopped.load(Ordering::SeqCst) {
                    let stream = match listener.accept() {
                        Ok((stream, _)) => stream,
                        // Waiting, or an error such as too many open files: either way, pause.
                        Err(_) => {
                            std::thread::sleep(ACCEPT_POLL);
                            continue;
                        }
                    };
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let slot = Slot(Arc::clone(&active));
                    let (manifest, sandboxes, ended, reports) = (
                        Arc::clone(&manifest),
                        sandboxes.clone(),
                        Arc::clone(&stopped),
                        Arc::clone(&reports),
                    );
                    // A thread that cannot start drops the stream and its slot.
                    let _ = std::thread::Builder::new().spawn(move || {
                        let _slot = slot;
                        let _ = connection(&stream, &manifest, &sandboxes, &ended, &reports);
                    });
                }
            })
        });
        let thread = match thread {
            Ok(thread) => thread,
            Err(e) => {
                let _ = std::fs::remove_file(path);
                return Err(e);
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            stop,
            submitted,
            thread: Some(thread),
        })
    }

    /// What the job's harness has submitted so far.
    pub(crate) fn submitted(&self) -> Submitted {
        self.submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Drop for JobSocket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// What one job's socket serves it from.
#[derive(Debug)]
struct Job {
    manifest: Manifest,
    forge_token: Option<ForgeToken>,
    /// The handles its manifest names, as the broker held them at admission.
    held: Held,
    /// Where its result goes when the harness submits one.
    submitted: Submission,
}

/// Where one job's gate refusals are reported.
#[derive(Debug)]
struct JobReports {
    egress: Reports,
    /// The git proxy's and the forge API's refusals (ADR-0046, ADR-0048).
    manifest: Reports,
    content: Reports,
    /// The role's refusals: a reader's call with tools (ADR-0061).
    role: Reports,
    /// A model provider or forge lost mid-job, which holds the task (ADR-0106).
    dependency: Reports,
}

/// What a refusal says was lost, if it says the model provider or the forge is gone rather than that
/// it refused (ADR-0106): the dependency's name and the code its unknown answer carries. A provider
/// still busy after every retry is lost; so is a forge that cannot be reached, answers what cannot be
/// read, or answers with a status it has no meaning for.
fn lost(reason: Option<&ReasonCode>) -> Option<(&'static str, &'static str)> {
    match reason.map(ReasonCode::as_str)? {
        "provider.overloaded" => Some(("model", MODEL_LOST)),
        "transport.unreachable" | "transport.bad_response" | "forge.upstream_status" => {
            Some(("forge", FORGE_LOST))
        }
        _ => None,
    }
}

/// The code a lost model provider's unknown answer carries (ADR-0106).
pub const MODEL_LOST: &str = "model.lost";
/// The code a lost forge's unknown answer carries (ADR-0106).
pub const FORGE_LOST: &str = "forge.lost";

/// Reports a loss the refusal `response` shows, if it shows one. The sandbox still gets the refusal.
fn report_lost(response: &Response, reports: &JobReports, ended: &AtomicBool) {
    if let Some((dependency, code)) = lost(response.reason.as_ref()) {
        reports
            .dependency
            .unknown(code, egress::inputs_digest(dependency), ended);
    }
}

/// Why a reader's call that defines a tool is refused (ADR-0037, ADR-0061).
pub const READER_TOOLS: &str = "model.reader_tools";

/// Why a call to a sink the manifest declares no flow to is refused (ADR-0055).
pub const FLOW_REFUSED: &str = "flow.not_declared";

/// The flow a call would carry the job's data along, if it carries any: a model call, a write to the
/// forge (a push, or anything under `/forge/`), or a request to a web host. A fetch through the git
/// proxy sends nothing of the job's, and a path the broker does not serve goes nowhere.
fn sink(head: &ProxyRequest) -> Option<&'static str> {
    if head.method == "CONNECT" || !head.target.starts_with('/') {
        return Some(flow::WEB);
    }
    if head.target.starts_with(pulls::PREFIX) {
        return Some(flow::FORGE);
    }
    if head.target.starts_with(git::PREFIX) {
        return git::route(&head.method, &head.target)
            .filter(|r| r.service == git::Service::ReceivePack)
            .map(|_| flow::FORGE);
    }
    let path = head.target.split('?').next().unwrap_or_default();
    (path.starts_with("/v1/messages") || path.starts_with("/v1/responses")).then_some(flow::MODEL)
}

fn connection(
    stream: &UnixStream,
    job: &Job,
    sandboxes: &Sandboxes,
    ended: &Arc<AtomicBool>,
    reports: &JobReports,
) -> io::Result<()> {
    let Job {
        manifest,
        forge_token,
        held,
        submitted,
    } = job;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let mut reader = BufReader::new(stream);
    let head = match read_head(&mut reader) {
        Ok(head) => head,
        Err((status, message)) => {
            return write_response(
                &mut &*stream,
                refusal(&OpenAi, status, "model.bad_request", message),
            );
        }
    };
    // Data may go only where the manifest's flows say (ADR-0055).
    let live = !ended.load(Ordering::SeqCst) && sandboxes.clock.now() <= manifest.deadline;
    if let Some(sink) = sink(&head).filter(|s| live && !manifest.data_flows.contains(*s)) {
        reports
            .manifest
            .deny(FLOW_REFUSED, egress::inputs_digest(sink), ended);
        let response = if sink == flow::MODEL {
            let adapter: &dyn Adapter = if head.target.starts_with("/v1/messages") {
                &Anthropic
            } else {
                &OpenAi
            };
            refusal(
                adapter,
                403,
                FLOW_REFUSED,
                "flow.not_declared: the job's manifest declares no data flow to a model",
            )
        } else {
            egress::refused(403, FLOW_REFUSED)
        };
        return write_response(&mut &*stream, response);
    }
    // Raw text the job's manifest names a released handle for (ADR-0063). It reads; it sends nothing.
    if head.method != "CONNECT" && head.target.starts_with(handles::PREFIX) {
        let response = if ended.load(Ordering::SeqCst) {
            egress::refused(403, "admission.ended")
        } else if sandboxes.clock.now() > manifest.deadline {
            egress::refused(403, "admission.expired")
        } else {
            handles::answer(&head, manifest, held, &reports.manifest, ended)
        };
        return write_response(&mut &*stream, response);
    }
    // The job's result, from `hyalos-submit` (ADR-0064). It goes to the engine as typed values only.
    if head.method != "CONNECT" && head.target.starts_with(submit::PREFIX) {
        let response = if ended.load(Ordering::SeqCst) {
            egress::refused(403, "admission.ended")
        } else if sandboxes.clock.now() > manifest.deadline {
            egress::refused(403, "admission.expired")
        } else {
            submit::answer(&head, &mut reader, &mut &*stream, manifest, submitted)
        };
        return write_response(&mut &*stream, response);
    }
    // Anything but a model call is a proxy request, for the egress gate (ADR-0042).
    if head.method == "CONNECT" || !head.target.starts_with('/') {
        let response = if ended.load(Ordering::SeqCst) {
            Some(egress::refused(403, "admission.ended"))
        } else if sandboxes.clock.now() > manifest.deadline {
            Some(egress::refused(403, "admission.expired"))
        } else {
            egress::proxy(
                head,
                &mut reader,
                stream,
                manifest,
                &sandboxes.egress,
                &reports.egress,
                ended,
            )?
        };
        return match response {
            Some(response) => write_response(&mut &*stream, response),
            None => Ok(()),
        };
    }
    // A forge call: a pull request (ADR-0048).
    if head.target.starts_with(pulls::PREFIX) {
        let response = if ended.load(Ordering::SeqCst) {
            egress::refused(403, "admission.ended")
        } else if sandboxes.clock.now() > manifest.deadline {
            egress::refused(403, "admission.expired")
        } else {
            pulls::answer(
                &head,
                &mut reader,
                &mut &*stream,
                manifest,
                forge_token.as_ref(),
                &sandboxes.forge,
                sandboxes.git.web_host(),
                &sandboxes.models.keys(),
                Gates {
                    manifest: &reports.manifest,
                    content: &reports.content,
                },
                ended,
            )
        };
        report_lost(&response, reports, ended);
        return write_response(&mut &*stream, response);
    }
    // A git call, for the git proxy (ADR-0046).
    if head.target.starts_with(git::PREFIX) {
        let mut response = if ended.load(Ordering::SeqCst) {
            egress::refused(403, "admission.ended")
        } else if sandboxes.clock.now() > manifest.deadline {
            egress::refused(403, "admission.expired")
        } else {
            git::answer(
                &head,
                &mut reader,
                &mut &*stream,
                manifest,
                forge_token.as_ref(),
                &sandboxes.git,
                &sandboxes.models.keys(),
                Gates {
                    manifest: &reports.manifest,
                    content: &reports.content,
                },
                ended,
            )
        };
        report_lost(&response, reports, ended);
        response.body = Box::new(UntilEnded {
            inner: response.body,
            ended: Arc::clone(ended),
        });
        return write_response(&mut &*stream, response);
    }
    let response = match read_body(head, &mut reader) {
        // A call read after the job ended never reaches a provider.
        Ok(request) if ended.load(Ordering::SeqCst) => {
            let adapter: &dyn Adapter = if request.target.starts_with("/v1/messages") {
                &Anthropic
            } else {
                &OpenAi
            };
            refusal(
                adapter,
                403,
                "admission.ended",
                "refused: the job has ended",
            )
        }
        Ok(request) => {
            let path = request
                .target
                .split('?')
                .next()
                .unwrap_or_default()
                .to_string();
            let mut response = sandboxes
                .models
                .answer(manifest, request, sandboxes.clock.as_ref());
            report_lost(&response, reports, ended);
            // A reader that defines a tool is refused, and the refusal is recorded (ADR-0061).
            if response.reason.as_ref().map(ReasonCode::as_str) == Some(READER_TOOLS) {
                reports
                    .role
                    .deny(READER_TOOLS, egress::inputs_digest(&path), ended);
            }
            // A stream is cut off when the job ends.
            response.body = Box::new(UntilEnded {
                inner: response.body,
                ended: Arc::clone(ended),
            });
            response
        }
        Err((status, message)) => refusal(&OpenAi, status, "model.bad_request", message),
    };
    write_response(&mut &*stream, response)
}

/// A response body that stops when its job ends.
struct UntilEnded {
    inner: Box<dyn Read + Send>,
    ended: Arc<AtomicBool>,
}

impl Read for UntilEnded {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.ended.load(Ordering::SeqCst) {
            return Err(io::Error::other("the job has ended"));
        }
        self.inner.read(buf)
    }
}

/// Gives back a connection's slot however its thread ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Reads one HTTP/1.1 model call: a request line, headers and a body of `content-length` bytes.
pub fn read_request<R: BufRead>(input: &mut R) -> Result<Request, (u16, &'static str)> {
    let head = read_head(input)?;
    if !head.target.starts_with('/') {
        return Err(BAD);
    }
    read_body(head, input)
}

const BAD: (u16, &str) = (400, "the request is not one the broker reads");

/// Reads a request line and headers. The target is as given: a path, `host:port` for a CONNECT, or
/// an absolute `http://` URL.
fn read_head<R: BufRead>(input: &mut R) -> Result<ProxyRequest, (u16, &'static str)> {
    let bad = BAD;
    let mut lines = Vec::new();
    let mut head = input.take(MAX_HEAD);
    loop {
        let mut line = Vec::new();
        head.read_until(b'\n', &mut line).map_err(|_| bad)?;
        if !line.ends_with(b"\n") {
            return Err((431, "the request head is too large or cut short"));
        }
        let line = String::from_utf8(line).map_err(|_| bad)?;
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            break;
        }
        if lines.len() > MAX_HEADERS {
            return Err((431, "the request has too many headers"));
        }
        lines.push(line);
    }
    let mut lines = lines.into_iter();
    let first = lines.next().ok_or(bad)?;
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad);
    };
    if !version.starts_with("HTTP/1.") || target.is_empty() {
        return Err(bad);
    }
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(bad)?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok(ProxyRequest {
        method: method.to_string(),
        target: target.to_string(),
        headers,
    })
}

/// Reads a model call's body, of `content-length` bytes.
fn read_body<R: BufRead>(
    head: ProxyRequest,
    input: &mut R,
) -> Result<Request, (u16, &'static str)> {
    let bad = BAD;
    let ProxyRequest {
        method,
        target,
        headers,
    } = head;
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
    };
    if get("transfer-encoding").is_some() {
        return Err((411, "the broker reads only a body with a content-length"));
    }
    let length = match get("content-length") {
        None => 0,
        Some(v) => v.parse::<usize>().map_err(|_| bad)?,
    };
    if length > MAX_BODY {
        return Err((413, "the request is larger than the broker reads"));
    }
    let mut body = vec![0; length];
    input
        .read_exact(&mut body)
        .map_err(|_| (400, "the request body was cut short"))?;
    Ok(Request {
        method,
        target,
        headers,
        body,
    })
}

/// Writes a response and its body as it arrives, then the connection closes: the body runs to the
/// end of the stream, so a streamed generation reaches the harness as it is made.
pub fn write_response<W: Write>(output: &mut W, response: Response) -> io::Result<()> {
    let content_type = if response.content_type.bytes().any(|b| b.is_ascii_control()) {
        "application/octet-stream"
    } else {
        response.content_type.as_str()
    };
    let phrase = match response.status {
        200..=299 => "OK",
        400..=499 => "Refused",
        _ => "Unavailable",
    };
    write!(
        output,
        "HTTP/1.1 {} {phrase}\r\ncontent-type: {content_type}\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
        response.status
    )?;
    let mut body = response.body;
    let mut buffer = [0u8; 8192];
    loop {
        let n = body.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
        output.flush()?;
    }
    output.flush()
}
