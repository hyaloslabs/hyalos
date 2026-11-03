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

use hyalos_types::wire::{ToBroker, ToEngine, WIRE_VERSION, WireError, read_frame, write_frame};
use hyalos_types::{Clock, JobId, Manifest, ManifestPublicKey, ReasonCode, SignedManifest};

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
}

impl Broker {
    pub fn new(key: ManifestPublicKey) -> Self {
        Self {
            admission: Admission::new(key),
            jobs: Jobs::default(),
            admitted: BTreeMap::new(),
        }
    }

    /// The manifest admitted for a live job, which is what the broker enforces for it.
    pub fn manifest(&self, job: JobId) -> Option<&Manifest> {
        self.admitted.get(&job)
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
                self.jobs.0.remove(&job);
                self.admitted.remove(&job);
                Ok(Some(Vec::new()))
            }
            ToBroker::StartJob {
                job,
                role,
                manifest,
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
                        self.admitted.insert(job, manifest);
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
pub fn serve<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
    clock: &dyn Clock,
) -> Result<Exit, ServeError> {
    let Some(ToBroker::Hello {
        version,
        manifest_key,
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
    let mut broker = Broker::new(manifest_key);
    while let Some(message) = read_frame::<_, ToBroker>(input)? {
        match broker.handle(message, clock)? {
            None => return Ok(Exit::Shutdown),
            Some(answers) => {
                for answer in answers {
                    write_frame(output, &answer)?;
                }
            }
        }
    }
    Ok(Exit::EngineClosed)
}
