//! The broker's side of the channel to the engine (ADR-0002, ADR-0035).
//!
//! [`Broker`] is the state: the engine's manifest key, which jobs are live and the manifests it has
//! admitted. It handles one message at a time and answers with typed values only. [`serve`] reads
//! frames, hands them to it and writes the answers. The broker takes its jobs from the engine and
//! guesses nothing: a job is live from the engine's `StartJob` to its `EndJob`.
//!
//! A manifest that is refused fails that job and the broker carries on. A frame that is not a message
//! the engine may send, or an engine that breaks the order (a second `Hello`, a job before `Hello`),
//! ends the channel, since the broker can no longer tell what the other end is.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use hyalos_types::wire::{
    MAX_PLUGIN_INPUT, PluginCall, ToBroker, ToEngine, WIRE_VERSION, WireError, read_frame,
    write_frame,
};
use hyalos_types::workflow::{TypedValue, output_is_valid};
use hyalos_types::{
    Clock, ForgeToken, JobId, Manifest, ManifestPublicKey, ReasonCode, Role, SignedManifest,
};

use crate::forge::Forge;
use crate::git::Git;
use crate::handles::{self, Held};
use crate::reader;
use crate::sandbox::{JobSocket, Sandboxes};
use crate::submit::Submitted;
use crate::{Admission, Credential, LiveJobs};

/// Runs a deterministic step's plugin (ADR-0102). This crate never links a WASM host (ADR-0086), so
/// the broker process's own code gives it one. The answer is the step's typed values, or a short
/// code.
pub trait Plugins: Send + Sync + fmt::Debug {
    fn run(&self, call: &PluginCall) -> Result<Vec<TypedValue>, ReasonCode>;
}

/// The jobs the engine has started and not ended.
#[derive(Debug, Default)]
struct Jobs(BTreeSet<JobId>);

impl LiveJobs for Jobs {
    fn is_live(&self, job: JobId) -> bool {
        self.0.contains(&job)
    }
}

/// What the broker knows once the engine has said hello.
#[derive(Debug)]
pub struct Broker {
    admission: Admission,
    jobs: Jobs,
    admitted: BTreeMap<JobId, Manifest>,
    /// The forge token the engine minted for each admitted job that needs one (ADR-0044). The broker
    /// holds no forge credential but these, and forgets each when its job ends.
    forge_tokens: BTreeMap<JobId, ForgeToken>,
    /// With sandboxes, each admitted job gets a socket its sandbox calls models through (ADR-0038).
    sandboxes: Option<Sandboxes>,
    sockets: BTreeMap<JobId, JobSocket>,
    /// Where facts the broker reports unprompted go: the egress gate's refusals (ADR-0042).
    reports: Option<Sender<ToEngine>>,
    /// Where the broker reads forge content for a job (ADR-0047).
    forge: Forge,
    /// The raw text the broker holds as handles, for the rest of its run (ADR-0063). The engine knows
    /// only their digests.
    held: Held,
    /// What runs deterministic steps' plugins, if this process has a runtime (ADR-0102).
    plugins: Option<Arc<dyn Plugins>>,
}

impl Broker {
    /// A broker that admits jobs and opens no sockets.
    pub fn new(key: ManifestPublicKey) -> Self {
        Self {
            admission: Admission::new(key),
            jobs: Jobs::default(),
            admitted: BTreeMap::new(),
            forge_tokens: BTreeMap::new(),
            sandboxes: None,
            sockets: BTreeMap::new(),
            reports: None,
            forge: Forge::github(),
            held: Held::new(),
            plugins: None,
        }
    }

    /// Runs deterministic steps' plugins with `plugins` (ADR-0102).
    pub fn with_plugins(mut self, plugins: Arc<dyn Plugins>) -> Self {
        self.plugins = Some(plugins);
        self
    }

    /// Reads forge content from `forge` instead of github.com. For tests; called only from code.
    pub fn with_forge(mut self, forge: Forge) -> Self {
        self.forge = forge;
        self
    }

    /// Forgets a job: its manifest, its token, its socket and its meter.
    fn end(&mut self, job: JobId) {
        self.jobs.0.remove(&job);
        self.admitted.remove(&job);
        self.forge_tokens.remove(&job);
        // Ends the socket first: calls still open are refused or cut off, and anything they settle
        // later is not added back to a forgotten meter.
        self.sockets.remove(&job);
        if let Some(sandboxes) = &self.sandboxes {
            sandboxes.models.meter().forget(job);
        }
    }

    /// Sends the egress gate's refusals for every job admitted from now on to `to`.
    pub fn report_to(mut self, to: Sender<ToEngine>) -> Self {
        self.reports = Some(to);
        self
    }

    /// A broker that serves each admitted job's sandbox on the socket the engine named for it.
    pub fn with_sandboxes(key: ManifestPublicKey, sandboxes: Sandboxes) -> Self {
        Self {
            sandboxes: Some(sandboxes),
            ..Self::new(key)
        }
    }

    /// The manifest admitted for a live job, which is what the broker enforces for it.
    pub fn manifest(&self, job: JobId) -> Option<&Manifest> {
        self.admitted.get(&job)
    }

    /// The forge token handed over with a live job's manifest, if it needed one.
    pub fn forge_token(&self, job: JobId) -> Option<&ForgeToken> {
        self.forge_tokens.get(&job)
    }

    /// Handles a message from the engine after `Hello`. `Ok(None)` means stop serving.
    pub fn handle(
        &mut self,
        message: ToBroker,
        clock: &dyn Clock,
    ) -> Result<Option<Vec<ToEngine>>, ServeError> {
        match message {
            ToBroker::Hello { .. } => Err(ServeError::Order),
            ToBroker::Shutdown => Ok(None),
            ToBroker::EndJob { job } => {
                self.end(job);
                Ok(Some(Vec::new()))
            }
            ToBroker::ReadIssue { job, number } => {
                let Some(manifest) = self.admitted.get(&job) else {
                    return refused(job, code("admission.not_live"));
                };
                if clock.now() > manifest.deadline {
                    self.end(job);
                    return refused(job, code("admission.expired"));
                }
                // Content that fails to read or parse fails this job, never the broker (ADR-0002).
                match self
                    .forge
                    .read_issue(manifest, self.forge_tokens.get(&job), number)
                {
                    Ok(issue) => Ok(Some(vec![ToEngine::Issue { job, issue }])),
                    Err(reason) => {
                        self.end(job);
                        refused(job, code(reason))
                    }
                }
            }
            ToBroker::HoldIssue { job, number } => {
                let Some(manifest) = self.admitted.get(&job) else {
                    return refused(job, code("admission.not_live"));
                };
                let result = if clock.now() > manifest.deadline {
                    Err("admission.expired")
                } else {
                    // Who wrote it, as permissions only, for policy to release it by (ADR-0103).
                    self.forge
                        .read_issue_and_writers(manifest, self.forge_tokens.get(&job), number)
                        .and_then(|(issue, writers)| {
                            handles::hold(&mut self.held, &issue).map(|handle| (handle, writers))
                        })
                };
                // As a read: one that fails fails its job, never the broker (ADR-0002).
                match result {
                    Ok((handle, writers)) => Ok(Some(vec![ToEngine::Held {
                        job,
                        handle,
                        writers,
                    }])),
                    Err(reason) => {
                        self.end(job);
                        refused(job, code(reason))
                    }
                }
            }
            ToBroker::Merge { job } => {
                let Some(manifest) = self.admitted.get(&job) else {
                    return refused(job, code("admission.not_live"));
                };
                let result = if clock.now() > manifest.deadline {
                    Err("admission.expired")
                } else {
                    self.forge.merge(manifest, self.forge_tokens.get(&job))
                };
                // One merge a job: done or refused, the job is over in the broker (ADR-0071).
                self.end(job);
                match result {
                    Ok(sha) => Ok(Some(vec![ToEngine::Merged { job, sha }])),
                    Err(reason) => refused(job, code(reason)),
                }
            }
            ToBroker::TakeResult { job } => {
                let Some(manifest) = self.admitted.get(&job) else {
                    return refused(job, code("admission.not_live"));
                };
                let submitted = match (&manifest.result, self.sockets.get(&job)) {
                    (None, _) => Submitted::Refused("submit.not_expected"),
                    (Some(_), None) => Submitted::Nothing,
                    (Some(_), Some(socket)) => socket.submitted(),
                };
                // No result, or one the broker refused, fails the job (ADR-0064).
                let reason = match submitted {
                    Submitted::Accepted(values) => {
                        return Ok(Some(vec![ToEngine::Values { job, values }]));
                    }
                    Submitted::Refused(reason) => reason,
                    Submitted::Nothing => "submit.missing",
                };
                self.end(job);
                refused(job, code(reason))
            }
            ToBroker::RunPlugin { job, call } => {
                // Only for a live deterministic job, and only a call within its limits (ADR-0102).
                let result = match (self.admitted.get(&job), &self.plugins) {
                    (None, _) => Err(code("admission.not_live")),
                    (Some(m), _) if m.role != Role::Deterministic => Err(code("plugin.wrong_role")),
                    _ if call.input.len() > MAX_PLUGIN_INPUT || !output_is_valid(&call.output) => {
                        Err(code("plugin.bad_call"))
                    }
                    (Some(_), None) => Err(code("plugin.no_runtime")),
                    (Some(_), Some(plugins)) => plugins.run(&call),
                };
                // A plugin that fails fails its job, never the broker.
                match result {
                    Ok(values) => Ok(Some(vec![ToEngine::Values { job, values }])),
                    Err(reason) => {
                        self.end(job);
                        refused(job, reason)
                    }
                }
            }
            ToBroker::Read { job, call } => {
                let Some(manifest) = self.admitted.get(&job) else {
                    return refused(job, code("admission.not_live"));
                };
                let result = match &self.sandboxes {
                    None => Err("model.no_provider"),
                    Some(sandboxes) => {
                        let (forge, token) = (&self.forge, self.forge_tokens.get(&job));
                        reader::run(manifest, &call, &sandboxes.models, clock, |number| {
                            forge.read_issue(manifest, token, number)
                        })
                    }
                };
                // A reader that fails fails its job, never the broker (ADR-0002, ADR-0061).
                match result {
                    Ok(values) => Ok(Some(vec![ToEngine::Values { job, values }])),
                    Err(reason) => {
                        self.end(job);
                        refused(job, code(reason))
                    }
                }
            }
            ToBroker::StartJob {
                job,
                role,
                manifest,
                socket,
                forge_token,
            } => {
                // A job is started once. The first admission stands.
                if self.jobs.0.contains(&job) {
                    return refused(job, code("admission.already_live"));
                }
                let Ok(signed) = SignedManifest::from_bytes(&manifest) else {
                    return refused(job, code("manifest.malformed"));
                };
                // The engine has said the job is live; admission still checks everything else.
                self.jobs.0.insert(job);
                match self.start(job, role, &signed, &socket, &forge_token, clock) {
                    Ok(manifest) => {
                        self.admitted.insert(job, manifest);
                        if let Some(token) = forge_token {
                            self.forge_tokens.insert(job, token);
                        }
                        Ok(Some(vec![ToEngine::Admitted { job }]))
                    }
                    Err(reason) => {
                        self.jobs.0.remove(&job);
                        refused(job, reason)
                    }
                }
            }
        }
    }

    /// Admits a job the engine has said is live, and opens its socket if this broker serves
    /// sandboxes. Nothing is kept for a job that is refused: its socket opens last.
    fn start(
        &mut self,
        job: JobId,
        role: Role,
        signed: &SignedManifest,
        socket: &str,
        forge_token: &Option<ForgeToken>,
        clock: &dyn Clock,
    ) -> Result<Manifest, ReasonCode> {
        let manifest = self
            .admission
            .admit(signed, &Credential { job, role }, &self.jobs, clock)
            .map_err(|refusal| refusal.reason())?;
        // A handle the manifest names must be one this broker holds (ADR-0063).
        let held =
            handles::for_manifest(&self.held, &manifest).ok_or_else(|| code("handle.not_held"))?;
        // A merge only with its approval, and never for a tainted job (ADR-0004, ADR-0071). An
        // approval with no merge is as wrong.
        let merging = manifest.actions.contains("merge");
        if merging != manifest.approval.is_some() || (merging && manifest.tainted) {
            return Err(code("admission.merge_unapproved"));
        }
        // A result the broker could not read a submission as (ADR-0064).
        if manifest
            .result
            .as_ref()
            .is_some_and(|r| !output_is_valid(r))
        {
            return Err(code("admission.bad_result"));
        }
        if let Some(sandboxes) = &self.sandboxes {
            let open = JobSocket::open(
                Path::new(socket),
                manifest.clone(),
                forge_token.clone(),
                held,
                sandboxes.clone(),
                self.reports.clone(),
            )
            .map_err(|_| code("broker.socket_unavailable"))?;
            self.sockets.insert(job, open);
        }
        Ok(manifest)
    }
}

/// The engine is answered that `job`'s request is refused, and why.
fn refused(job: JobId, reason: ReasonCode) -> Result<Option<Vec<ToEngine>>, ServeError> {
    Ok(Some(vec![ToEngine::Refused { job, reason }]))
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

/// Why serving ended other than by the engine closing the channel or saying stop.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeError {
    /// A frame could not be read or written.
    #[error("{0}")]
    Wire(WireError),
    /// The first message was not `Hello`, or `Hello` came again.
    #[error("the engine sent messages out of order")]
    Order,
    /// The engine speaks another version of the protocol.
    #[error("the engine speaks protocol {engine}, and this broker speaks {WIRE_VERSION}")]
    Version { engine: u32 },
}

impl From<WireError> for ServeError {
    fn from(e: WireError) -> Self {
        ServeError::Wire(e)
    }
}

/// How serving ended without a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The engine said to stop.
    Shutdown,
    /// The engine closed the channel.
    EngineClosed,
}

/// How often the writer looks for the end of serving.
const WRITER_POLL: Duration = Duration::from_millis(50);

/// Reads the engine's `Hello`, answers `Ready`, then serves until told to stop or the channel ends.
/// With `sandboxes`, each admitted job's sandbox is served on its socket, and with `plugins`
/// deterministic steps' plugins are run (ADR-0102). Answers and the reports the sandboxes' requests
/// give rise to are written by one writer, in the order made.
pub fn serve_with_plugins<R: Read, W: Write + Send>(
    input: &mut R,
    output: &mut W,
    clock: &dyn Clock,
    sandboxes: Option<Sandboxes>,
    plugins: Option<Arc<dyn Plugins>>,
) -> Result<Exit, ServeError> {
    let Some(ToBroker::Hello {
        version,
        manifest_key,
        forge,
    }) = read_frame::<_, ToBroker>(input)?
    else {
        return Err(ServeError::Order);
    };
    if version != WIRE_VERSION {
        return Err(ServeError::Version { engine: version });
    }
    write_frame(
        output,
        &ToEngine::Ready {
            version: WIRE_VERSION,
        },
    )?;
    let (to_writer, from_broker) = mpsc::channel::<ToEngine>();
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let done = &done;
        let writer = scope.spawn(move || -> Result<(), WireError> {
            loop {
                match from_broker.recv_timeout(WRITER_POLL) {
                    Ok(message) => write_frame(output, &message)?,
                    Err(RecvTimeoutError::Timeout) if done.load(Ordering::SeqCst) => {
                        while let Ok(message) = from_broker.try_recv() {
                            write_frame(output, &message)?;
                        }
                        return Ok(());
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }
        });
        // The forge the engine names in `Hello` (ADR-0050), github.com otherwise.
        let sandboxes = sandboxes.map(|mut s| {
            if let Some(bases) = &forge {
                s.git = Git::with_base(&bases.git);
                s.forge = Forge::with_base(&bases.api);
            }
            s
        });
        let mut broker = match sandboxes {
            Some(sandboxes) => Broker::with_sandboxes(manifest_key, sandboxes),
            None => Broker::new(manifest_key),
        }
        .report_to(to_writer.clone());
        if let Some(bases) = &forge {
            broker = broker.with_forge(Forge::with_base(&bases.api));
        }
        if let Some(plugins) = plugins {
            broker = broker.with_plugins(plugins);
        }
        let served = (|| {
            while let Some(message) = read_frame::<_, ToBroker>(input)? {
                match broker.handle(message, clock)? {
                    None => return Ok(Exit::Shutdown),
                    Some(answers) => {
                        for answer in answers {
                            // The writer has stopped on an error, which it returns below.
                            if to_writer.send(answer).is_err() {
                                return Ok(Exit::EngineClosed);
                            }
                        }
                    }
                }
            }
            Ok(Exit::EngineClosed)
        })();
        // Closes every job's socket before the writer finishes what was queued.
        drop(broker);
        done.store(true, Ordering::SeqCst);
        let written = writer
            .join()
            .unwrap_or(Err(WireError::Io(std::io::ErrorKind::Other)));
        match (served, written) {
            (Err(e), _) => Err(e),
            (Ok(_), Err(e)) => Err(ServeError::Wire(e)),
            (Ok(exit), Ok(())) => Ok(exit),
        }
    })
}
