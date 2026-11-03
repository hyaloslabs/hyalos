//! The engine's loop over the log: submit an event, append it, project it (ADR-0003, ADR-0019).

use std::fmt;
use std::path::Path;

use hyalos_log::{
    Anchor, CompactError, Head, Log, LogError, LogKey, NewEvent, StoredEvent, VerifyError,
};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_types::event::SCHEMA;
use hyalos_types::snapshot::{STATE_VERSION, StateImage};
use hyalos_types::{
    Answer, Event, EventError, HoldId, JobId, Origin, ReasonCode, Resolution, Schema, Timestamp,
};

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
    /// The log's latest snapshot could not be used as the state to start from (ADR-0026).
    Snapshot(String),
    /// The log was not compacted. It is as it was.
    Compact(CompactError<String>),
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
            EngineError::Snapshot(e) => write!(f, "the log's snapshot cannot be used: {e}"),
            EngineError::Compact(e) => write!(f, "{e}"),
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
    replay_from(schema, State::default(), events)
}

/// Carries on from `state`, which is the state a snapshot stood for, through the events that follow
/// it (ADR-0026). With the empty state it is a replay from the beginning.
pub fn replay_from(
    schema: &Schema,
    mut state: State,
    events: &[StoredEvent],
) -> Result<State, EngineError> {
    for e in events {
        let event = Event::decode_with(schema, &e.kind, e.schema_version, &e.payload)
            .map_err(|error| EngineError::Event { seq: e.seq, error })?;
        state
            .apply(e.seq, e.at, e.origin, &event)
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

/// The state a snapshot stood for.
fn state_of(anchor: &Anchor) -> Result<State, String> {
    let image =
        StateImage::decode(anchor.state_version, &anchor.state).map_err(|e| e.to_string())?;
    if image.last_seq != anchor.seq {
        return Err(format!(
            "it stands for event {} but its state ends at event {}",
            anchor.seq, image.last_seq
        ));
    }
    State::from_image(image)
}

/// The log's state: its latest snapshot, if it has one, then the events that follow.
fn restored_state(schema: &Schema, log: &Log) -> Result<State, EngineError> {
    let base = match log.latest_anchor()? {
        Some(anchor) => state_of(&anchor).map_err(EngineError::Snapshot)?,
        None => State::default(),
    };
    replay_from(schema, base, &log.events()?)
}

/// What a compaction did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compaction {
    /// The last event moved to the archive, which the new snapshot stands for.
    pub head: Head,
    /// How many events were moved.
    pub events: u64,
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

/// What happened when a held gate was asked again (ADR-0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reevaluation {
    /// The gate answered allow in time, and the hold is released.
    Released(Head),
    /// The gate did not allow in time, or refused. Nothing was written and the task is still held.
    StillHeld,
}

/// Who decided a hold at the tier of its action (ADR-0012). A fresh allow from the gate is not a
/// decision; it goes through [`Engine::reevaluate_hold`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decider {
    Policy,
    Verifier,
    Human,
}

impl Decider {
    fn resolution(self) -> Resolution {
        match self {
            Decider::Policy => Resolution::Policy,
            Decider::Verifier => Resolution::Verifier,
            Decider::Human => Resolution::Human,
        }
    }
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
    /// log is verified against `key` before it is read, which costs a pass over every event.
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
        // No "does it exist?" test before opening: that would leave a gap in which the file could
        // appear and be opened unverified. Open (creating it if it is new), then verify that same
        // open file, not the path again. A log that is new and empty verifies trivially.
        let public = key.public();
        let log = Log::open(path, key)?;
        log.verify(&public).map_err(EngineError::Verify)?;
        let state = restored_state(schema, &log)?;
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

    /// Where the latest snapshot stands, if the log was ever compacted: the events up to and including
    /// this one are in archives, and the log holds the ones after it.
    pub fn snapshot_head(&self) -> Result<Option<Head>, EngineError> {
        Ok(self.log.latest_anchor()?.map(|a| a.head()))
    }

    /// Moves every event to a new archive file and replaces them in the log with a signed snapshot of
    /// the state (ADR-0026). The archive is a verified copy of the whole log; `archive` must not exist.
    /// Nothing is deleted: the events are in the archive, and the chain carries on from the snapshot
    /// with every hash unchanged.
    ///
    /// The snapshot is only taken if the log, replayed, gives exactly this engine's state, and once it
    /// is written the log is verified and the state rebuilt from the snapshot is compared with the live
    /// one. Either failing is an error, not a log that quietly differs. `at` is when this is done and
    /// may not be before the last event.
    pub fn compact(&mut self, at: Timestamp, archive: &Path) -> Result<Compaction, EngineError> {
        if self.poisoned {
            return Err(EngineError::Poisoned);
        }
        if let Some(previous) = self.state.last_at()
            && at < previous
        {
            return Err(EngineError::Rule(RuleViolation::TimeWentBackwards {
                previous,
                at,
            }));
        }
        let archived = self
            .log
            .archive(archive)
            .map_err(|e| EngineError::Compact(e.map_snapshot(|never| match never {})))?;
        let (schema, live) = (self.schema, &self.state);
        let mut moved = 0u64;
        self.log
            .compact(archived, at, |prior, events| {
                moved = events.len() as u64;
                let base = match prior {
                    Some(anchor) => state_of(anchor)?,
                    None => State::default(),
                };
                let replayed = replay_from(schema, base, events)
                    .map_err(|e| format!("the log does not replay: {e}"))?;
                if &replayed != live {
                    return Err(
                        "the engine's state is not what its log replays to, so it will not be snapshotted"
                            .to_string(),
                    );
                }
                let image = replayed
                    .to_image()
                    .map_err(|job| format!("job {job}'s unknown answer has no hold yet"))?;
                let bytes = image.encode().map_err(|e| e.to_string())?;
                Ok((STATE_VERSION, bytes))
            })
            .map_err(EngineError::Compact)?;

        // The compaction has happened. Prove the log still verifies and still means the same thing,
        // and stop if it does not: the archive holds every event, so nothing is lost.
        self.log
            .verify(&self.log.public_key())
            .map_err(EngineError::Verify)?;
        if restored_state(self.schema, &self.log)? != self.state {
            self.poisoned = true;
            return Err(EngineError::Snapshot(
                "the state rebuilt from the new snapshot differs from the live state; restart the engine and restore from the archive".into(),
            ));
        }
        Ok(Compaction {
            head: archived,
            events: moved,
        })
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
        // A broker is told it may not send this at all (ADR-0024) before anyone is told about the API.
        if matches!(event, Event::HoldResolved { .. }) && event.may_come_from(origin) {
            return Err(EngineError::Rule(RuleViolation::ResolveThroughTheEngineApi));
        }
        self.submit_checked(at, origin, event)
    }

    /// [`Engine::submit`] without the refusal of `hold.resolved`, for the two methods that justify one.
    fn submit_checked(
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
        self.state
            .check_stamped(at, origin, &event)
            .map_err(EngineError::Rule)?;
        let payload = event
            .encode()
            .map_err(|error| EngineError::Event { seq: 0, error })?;
        let head = self
            .log
            .append(at, origin, event.kind(), self.schema.current, &payload)?;
        if self.state.apply(head.seq, at, origin, &event).is_err() {
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
        events: &[(Origin, Event)],
    ) -> Result<Head, EngineError> {
        if self.poisoned {
            return Err(EngineError::Poisoned);
        }
        if events
            .iter()
            .any(|(o, e)| matches!(e, Event::HoldResolved { .. }) && e.may_come_from(*o))
        {
            return Err(EngineError::Rule(RuleViolation::ResolveThroughTheEngineApi));
        }
        // Checked on a copy, so a sequence that fails half way leaves the real state untouched.
        let mut next = self.state.clone();
        for (origin, event) in events {
            let seq = next.last_seq() + 1;
            next.apply(seq, at, *origin, event)
                .map_err(EngineError::Rule)?;
        }
        if next.awaiting_hold() {
            return Err(EngineError::Rule(RuleViolation::UnknownNeedsHold));
        }
        let payloads = events
            .iter()
            .map(|(_, e)| e.encode())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| EngineError::Event { seq: 0, error })?;
        let batch: Vec<NewEvent<'_>> = events
            .iter()
            .zip(&payloads)
            .map(|((origin, e), payload)| NewEvent {
                at,
                origin: *origin,
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
                // The gate's answer is recorded as the origin that gave it. Ending the job and opening
                // the hold are the engine's own decisions, so they are recorded as the engine's.
                let head = self.submit_all(
                    at,
                    &[
                        (origin, answered(Answer::Unknown, reason)),
                        (
                            Origin::Engine,
                            Event::JobEnded {
                                job,
                                outcome: hyalos_types::JobOutcome::Held,
                            },
                        ),
                        (
                            Origin::Engine,
                            Event::HoldOpened {
                                task,
                                gate: request.gate,
                                inputs: request.inputs,
                                reason: why,
                                stakes: request.stakes,
                            },
                        ),
                    ],
                )?;
                Ok(Ruling::Held(HoldId::from_seq(head.seq)))
            }
        }
    }
    /// Asks a held gate again. `outcome` is what [`hyalos_policy::ask`] returned for `request`, which
    /// must be the gate and inputs the hold holds. Only an on-time allow releases the hold, recorded
    /// as resolved by re-evaluation with the gate's reason. Unknown, a timeout and a refusal leave it
    /// open and write nothing (ADR-0009, ADR-0025). A hold whose stakes need a decision cannot be
    /// released this way and is refused.
    pub fn reevaluate_hold(
        &mut self,
        at: Timestamp,
        hold: HoldId,
        request: &GateRequest,
        outcome: Outcome,
    ) -> Result<Reevaluation, EngineError> {
        let held = self.open_hold(hold)?;
        if held.gate != request.gate || held.inputs != request.inputs {
            return Err(EngineError::Rule(RuleViolation::ReevaluatedAnotherRequest(
                hold,
            )));
        }
        match outcome {
            Outcome::Proceed { reason } => {
                let head = self.submit_checked(
                    at,
                    Origin::Engine,
                    Event::HoldResolved {
                        hold,
                        by: Resolution::Reevaluated,
                        reason,
                    },
                )?;
                Ok(Reevaluation::Released(head))
            }
            Outcome::Refuse { .. } | Outcome::Hold { .. } => Ok(Reevaluation::StillHeld),
        }
    }

    /// Records a decision by the tier of the held action: policy for a routine hold, the verifier
    /// agent for an escalated one, a human for an irreversible one or one touching credentials,
    /// policy, publishing or a capability grant (ADR-0012, ADR-0025). A decider not allowed at the
    /// hold's stakes is refused and nothing is written. `reason` is a short code, never free text.
    pub fn decide_hold(
        &mut self,
        at: Timestamp,
        hold: HoldId,
        decider: Decider,
        reason: ReasonCode,
    ) -> Result<Head, EngineError> {
        self.submit_checked(
            at,
            Origin::Engine,
            Event::HoldResolved {
                hold,
                by: decider.resolution(),
                reason,
            },
        )
    }

    fn open_hold(&self, hold: HoldId) -> Result<&crate::Hold, EngineError> {
        match self.state.holds().get(&hold) {
            None => Err(EngineError::Rule(RuleViolation::UnknownHold(hold))),
            Some(h) if h.resolved.is_some() => {
                Err(EngineError::Rule(RuleViolation::HoldNotOpen(hold)))
            }
            Some(h) => Ok(h),
        }
    }
}
