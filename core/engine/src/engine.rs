//! The engine's loop over the log: submit an event, append it, project it (ADR-0003, ADR-0019).

use std::fmt;
use std::path::Path;

use hyalos_log::{Head, Log, LogError, LogKey, NewEvent, StoredEvent, VerifyError, verify};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_types::event::SCHEMA;
use hyalos_types::{Answer, Event, EventError, HoldId, JobId, Origin, Schema, Timestamp};

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
    /// The schema table the engine was given is inconsistent (ADR-0023).
    Schema(String),
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
            EngineError::Schema(e) => write!(f, "the event schema table is not valid: {e}"),
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
    replay_with(&SCHEMA, events)
}

/// [`replay`] under another schema: events written at an older version are upcast as they are read
/// (ADR-0023).
pub fn replay_with(schema: &Schema, events: &[StoredEvent]) -> Result<State, EngineError> {
    let mut state = State::default();
    for e in events {
        let event = Event::decode_with(schema, &e.kind, e.schema_version, &e.payload)
            .map_err(|error| EngineError::Event { seq: e.seq, error })?;
        state
            .apply(e.seq, e.at, &event)
            .map_err(|violation| EngineError::Replay {
                seq: e.seq,
                violation,
            })?;
    }
    if let Some(job) = state.unfinished_hold() {
        return Err(EngineError::Replay {
            seq: state.last_seq(),
            violation: RuleViolation::UnfinishedHold(job),
        });
    }
    Ok(state)
}

/// What the engine decided about a gate's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ruling {
    /// The request may go ahead.
    Proceed,
    /// This request is refused; the job carries on.
    Refused,
    /// The job has ended held and this hold is the open decision (ADR-0009).
    Held(HoldId),
}

/// The log together with the state projected from it.
#[derive(Debug)]
pub struct Engine {
    log: Log,
    state: State,
    schema: &'static Schema,
    poisoned: bool,
}

impl Engine {
    /// Opens the log at `path` (creating it if it is new) and rebuilds the state from it. An existing
    /// log is verified against `key` first, which costs a pass over every event.
    pub fn open(path: &Path, key: LogKey) -> Result<Self, EngineError> {
        Self::open_with(path, key, &SCHEMA)
    }

    /// [`Engine::open`] under another event schema, so a future version can be run against a log a
    /// past one wrote (ADR-0023). Old events are upcast in memory; the log is not rewritten, and new
    /// events are written at `schema.current`. The table is checked first.
    pub fn open_with(
        path: &Path,
        key: LogKey,
        schema: &'static Schema,
    ) -> Result<Self, EngineError> {
        schema.validate().map_err(EngineError::Schema)?;
        if path.exists() {
            verify(path, &key.public(), None).map_err(EngineError::Verify)?;
        }
        let log = Log::open(path, key)?;
        let state = replay_with(schema, &log.events()?)?;
        Ok(Self {
            log,
            state,
            schema,
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
        if matches!(
            event,
            Event::GateAnswered {
                answer: Answer::Unknown,
                ..
            }
        ) {
            return Err(EngineError::Rule(RuleViolation::UnknownNeedsHold));
        }
        self.state.check(&event).map_err(EngineError::Rule)?;
        let payload = event
            .encode()
            .map_err(|error| EngineError::Event { seq: 0, error })?;
        let head = self
            .log
            .append(at, origin, event.kind(), self.schema.current, &payload)?;
        if self.state.apply(head.seq, at, &event).is_err() {
            self.poisoned = true;
            return Err(EngineError::Poisoned);
        }
        Ok(head)
    }
    /// Checks a sequence of events against the state, one after another, then writes all of them in
    /// one transaction: every event is written or none is. The sequence must leave nothing owed, so an
    /// unknown answer is accepted only together with its job ending and its hold.
    pub fn submit_all(
        &mut self,
        at: Timestamp,
        origin: Origin,
        events: &[Event],
    ) -> Result<Head, EngineError> {
        if self.poisoned {
            return Err(EngineError::Poisoned);
        }
        // Checked on a copy, so a sequence that fails half way leaves the real state untouched.
        let mut next = self.state.clone();
        for event in events {
            let seq = next.last_seq() + 1;
            next.apply(seq, at, event).map_err(EngineError::Rule)?;
        }
        if next.awaiting_hold() {
            return Err(EngineError::Rule(RuleViolation::UnknownNeedsHold));
        }
        let payloads = events
            .iter()
            .map(|e| e.encode())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| EngineError::Event { seq: 0, error })?;
        let batch: Vec<NewEvent<'_>> = events
            .iter()
            .zip(&payloads)
            .map(|(e, payload)| NewEvent {
                at,
                origin,
                kind: e.kind(),
                schema_version: self.schema.current,
                payload,
            })
            .collect();
        let head = self.log.append_all(&batch)?;
        if head.seq != next.last_seq() {
            self.poisoned = true;
            return Err(EngineError::Poisoned);
        }
        self.state = next;
        Ok(head)
    }

    /// Records a gate's outcome for a running job and says what happens next (ADR-0009, ADR-0020).
    /// An allow or deny is one event. A hold is three, written together: the unknown answer, the job
    /// ending held, and the hold opening. So no crash can leave a task that looks open after a gate
    /// failed to say yes.
    pub fn rule_gate(
        &mut self,
        at: Timestamp,
        origin: Origin,
        job: JobId,
        request: &GateRequest,
        outcome: Outcome,
    ) -> Result<Ruling, EngineError> {
        let task = self
            .state
            .jobs()
            .get(&job)
            .map(|j| j.task)
            .ok_or(EngineError::Rule(RuleViolation::UnknownJob(job)))?;
        let answered = |answer, reason| Event::GateAnswered {
            job,
            gate: request.gate,
            inputs: request.inputs,
            answer,
            reason,
        };
        match outcome {
            Outcome::Proceed { reason } => {
                self.submit(at, origin, answered(Answer::Allow, reason))?;
                Ok(Ruling::Proceed)
            }
            Outcome::Refuse { reason } => {
                self.submit(at, origin, answered(Answer::Deny, reason))?;
                Ok(Ruling::Refused)
            }
            Outcome::Hold { reason, why } => {
                let head = self.submit_all(
                    at,
                    origin,
                    &[
                        answered(Answer::Unknown, reason),
                        Event::JobEnded {
                            job,
                            outcome: hyalos_types::JobOutcome::Held,
                        },
                        Event::HoldOpened {
                            task,
                            gate: request.gate,
                            inputs: request.inputs,
                            reason: why,
                        },
                    ],
                )?;
                Ok(Ruling::Held(HoldId::from_seq(head.seq)))
            }
        }
    }
}
