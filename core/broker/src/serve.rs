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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use hyalos_types::wire::{ToBroker, ToEngine, WIRE_VERSION, WireError, read_frame, write_frame};
use hyalos_types::{
    Clock, ForgeToken, JobId, Manifest, ManifestPublicKey, ReasonCode, SignedManifest,
};

use crate::forge::Forge;
use crate::git::Git;
use crate::reader;
use crate::sandbox::{JobSocket, Sandboxes};
use crate::{Admission, Credential, LiveJobs};

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
        }
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
                    return Ok(Some(vec![ToEngine::Refused {
                        job,
                        reason: code("admission.not_live"),
                    }]));
                };
                if clock.now() > manifest.deadline {
                    self.end(job);
                    return Ok(Some(vec![ToEngine::Refused {
                        job,
                        reason: code("admission.expired"),
                    }]));
                }
                // Content that fails to read or parse fails this job, never the broker (ADR-0002).
                match self
                    .forge
                    .read_issue(manifest, self.forge_tokens.get(&job), number)
                {
                    Ok(issue) => Ok(Some(vec![ToEngine::Issue { job, issue }])),
                    Err(reason) => {
                        self.end(job);
                        Ok(Some(vec![ToEngine::Refused {
                            job,
                            reason: code(reason),
                        }]))
                    }
                }
            }
            ToBroker::Read { job, call } => {
                let Some(manifest) = self.admitted.get(&job) else {
                    return Ok(Some(vec![ToEngine::Refused {
                        job,
                        reason: code("admission.not_live"),
                    }]));
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
                        Ok(Some(vec![ToEngine::Refused {
                            job,
                            reason: code(reason),
                        }]))
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
                let refuse = |reason: ReasonCode| Ok(Some(vec![ToEngine::Refused { job, reason }]));
                // A job is started once. The first admission stands.
                if self.jobs.0.contains(&job) {
                    return refuse(code("admission.already_live"));
                }
                let Ok(signed) = SignedManifest::from_bytes(&manifest) else {
                    return refuse(code("manifest.malformed"));
                };
                // The engine has said the job is live; admission still checks everything else.
                self.jobs.0.insert(job);
                match self
                    .admission
                    .admit(&signed, &Credential { job, role }, &self.jobs, clock)
                {
                    Ok(manifest) => {
                        if let Some(sandboxes) = &self.sandboxes {
                            match JobSocket::open(
                                Path::new(&socket),
                                manifest.clone(),
                                forge_token.clone(),
                                sandboxes.clone(),
                                self.reports.clone(),
                            ) {
                                Ok(open) => {
                                    self.sockets.insert(job, open);
                                }
                                Err(_) => {
                                    self.jobs.0.remove(&job);
                                    return refuse(code("broker.socket_unavailable"));
                                }
                            }
                        }
                        self.admitted.insert(job, manifest);
                        if let Some(token) = forge_token {
                            self.forge_tokens.insert(job, token);
                        }
                        Ok(Some(vec![ToEngine::Admitted { job }]))
                    }
                    Err(refusal) => {
                        self.jobs.0.remove(&job);
                        refuse(refusal.reason())
                    }
                }
            }
        }
    }
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

/// Why serving ended other than by the engine closing the channel or saying stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeError {
    /// A frame could not be read or written.
    Wire(WireError),
    /// The first message was not `Hello`, or `Hello` came again.
    Order,
    /// The engine speaks another version of the protocol.
    Version { engine: u32 },
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServeError::Wire(e) => write!(f, "{e}"),
            ServeError::Order => f.write_str("the engine sent messages out of order"),
            ServeError::Version { engine } => write!(
                f,
                "the engine speaks protocol {engine}, and this broker speaks {WIRE_VERSION}"
            ),
        }
    }
}

impl std::error::Error for ServeError {}

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

/// Reads the engine's `Hello`, answers `Ready`, then serves until told to stop or the channel ends.
pub fn serve<R: Read, W: Write + Send>(
    input: &mut R,
    output: &mut W,
    clock: &dyn Clock,
) -> Result<Exit, ServeError> {
    serve_with(input, output, clock, None)
}

/// How often the writer looks for the end of serving.
const WRITER_POLL: Duration = Duration::from_millis(50);

/// As [`serve`], and with `sandboxes`, each admitted job's sandbox is served on its socket. Answers and
/// the reports the sandboxes' requests give rise to are written by one writer, in the order made.
pub fn serve_with<R: Read, W: Write + Send>(
    input: &mut R,
    output: &mut W,
    clock: &dyn Clock,
    sandboxes: Option<Sandboxes>,
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
