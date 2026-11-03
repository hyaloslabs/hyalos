//! The engine's loop over the log: submit an event, append it, project it (ADR-0003, ADR-0019).

use std::fmt;
use std::path::Path;

use hyalos_log::{Head, Log, LogError, LogKey, StoredEvent, VerifyError, verify};
use hyalos_types::event::SCHEMA_VERSION;
use hyalos_types::{Event, EventError, Origin, Timestamp};

use crate::state::{RuleViolation, State};

#[derive(Debug)]
pub enum EngineError {
    Log(LogError),
    /// The existing log does not verify against the engine's key, so it is not loaded.
    Verify(VerifyError),
    /// A stored event could not be read back.
    Event {
        seq: u64,
        error: EventError,
    },
    /// An event was refused because it is not legal for the current state.
    Rule(RuleViolation),
    /// A stored event is not legal for the state before it. The log is damaged or was written by a
    /// buggy engine.
    Replay {
        seq: u64,
        violation: RuleViolation,
    },
    /// An event was appended but could not be projected. This cannot happen, since it was checked
    /// first. The engine stops accepting events rather than continue with state it cannot trust.
    Poisoned,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Log(e) => write!(f, "{e}"),
            EngineError::Verify(e) => write!(f, "the event log failed verification: {e}"),
            EngineError::Event { seq, error } => write!(f, "event {seq}: {error}"),
            EngineError::Rule(v) => write!(f, "refused: {v}"),
            EngineError::Replay { seq, violation } => {
                write!(
                    f,
                    "event {seq} is not legal where it sits in the log: {violation}"
                )
            }
            EngineError::Poisoned => {
                write!(
                    f,
                    "the engine's state no longer matches its log; restart it"
                )
            }
        }
    }
}

impl std::error::Error for EngineError {}

impl From<LogError> for EngineError {
    fn from(e: LogError) -> Self {
        EngineError::Log(e)
    }
}

/// Rebuilds the state from stored events. The same events always give an equal state.
pub fn replay(events: &[StoredEvent]) -> Result<State, EngineError> {
    let mut state = State::default();
    for e in events {
        let event = Event::decode(&e.kind, e.schema_version, &e.payload)
            .map_err(|error| EngineError::Event { seq: e.seq, error })?;
        state
            .apply(e.seq, e.at, &event)
            .map_err(|violation| EngineError::Replay {
                seq: e.seq,
                violation,
            })?;
    }
    Ok(state)
}

/// The log together with the state projected from it.
#[derive(Debug)]
pub struct Engine {
    log: Log,
    state: State,
    poisoned: bool,
}

impl Engine {
    /// Opens the log at `path` (creating it if it is new) and rebuilds the state from it. An existing
    /// log is verified against `key` first, which costs a pass over every event.
    pub fn open(path: &Path, key: LogKey) -> Result<Self, EngineError> {
        if path.exists() {
            verify(path, &key.public(), None).map_err(EngineError::Verify)?;
        }
        let log = Log::open(path, key)?;
        let state = replay(&log.events()?)?;
        Ok(Self {
            log,
            state,
            poisoned: false,
        })
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn head(&self) -> Result<Option<Head>, EngineError> {
        Ok(self.log.head()?)
    }

    /// Checks `event` against the state, appends it, then projects it. An illegal event is refused
    /// before anything is written, so the log never holds an event that replay would reject.
    pub fn submit(
        &mut self,
        at: Timestamp,
        origin: Origin,
        event: Event,
    ) -> Result<Head, EngineError> {
        if self.poisoned {
            return Err(EngineError::Poisoned);
        }
        self.state.check(&event).map_err(EngineError::Rule)?;
        let payload = event
            .encode()
            .map_err(|error| EngineError::Event { seq: 0, error })?;
        let head = self
            .log
            .append(at, origin, event.kind(), SCHEMA_VERSION, &payload)?;
        if self.state.apply(head.seq, at, &event).is_err() {
            self.poisoned = true;
            return Err(EngineError::Poisoned);
        }
        Ok(head)
    }
}
