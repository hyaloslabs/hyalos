//! Learnings over the event log (ADR-0112): what the log's typed records say happened, grouped by
//! what was learned, each with its provenance and the events it was learned from. It reads the log
//! with the engine's public key alone, checks it first, and writes nothing.
//!
//! A learning holds only typed values from the log: outcomes, gates, answers, deciders and reason
//! codes, which Hyalos writes from closed sets, and the typed values a job returned. No text from the
//! forge or a model is in the log (ADR-0003), so none is in a learning.

use std::path::Path;

use hyalos_log::{PublicKey, StoredEvent, read_verified};
use hyalos_types::provenance::Provenance;
use hyalos_types::{
    Answer, Digest, Event, Gate, JobId, JobOutcome, Origin, ReasonCode, Resolution, Timestamp,
    TypedValue,
};

use hyalos_types::event::SCHEMA;

use crate::engine::state_from;
use crate::{EngineError, State};

/// Most source events one learning lists, the latest ones. `count` says how many there were.
pub const MAX_SOURCES: usize = 20;

/// What was learned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lesson {
    /// Jobs ended this way, for this reason.
    JobEnded {
        outcome: JobOutcome,
        reason: ReasonCode,
    },
    /// A gate refused a request, or could not decide it. Allowing is not a lesson.
    GateAnswered {
        gate: Gate,
        answer: Answer,
        reason: ReasonCode,
    },
    /// Holds were released by this decider, for this reason.
    HoldResolved { by: Resolution, reason: ReasonCode },
    /// A held request was put to its gate again and refused.
    HoldRefused { reason: ReasonCode },
    /// Held tasks were ended by this decider, for this reason.
    TaskCancelled { by: Resolution, reason: ReasonCode },
    /// The typed values one job returned. A model chose them, and the log does not say whether it
    /// had read tainted input, so they are `ModelFromTainted` until something that knows says
    /// otherwise.
    Values { job: JobId, values: Vec<TypedValue> },
}

/// One event a learning was learned from: where it stands in the chain and who reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub seq: u64,
    /// The event's hash in the chain, so a reader can check it with `hyalos log verify`.
    pub hash: Digest,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Learning {
    pub lesson: Lesson,
    /// Who wrote what it holds (ADR-0005): `Operator` for Hyalos's own records, `ModelFromTainted`
    /// for values a model chose.
    pub provenance: Provenance,
    /// How many events it was learned from.
    pub count: u64,
    pub first_at: Timestamp,
    pub last_at: Timestamp,
    /// The latest of those events, oldest first, at most [`MAX_SOURCES`].
    pub sources: Vec<Source>,
}

/// Every learning in the log, the most recent first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Learnings {
    /// The first event learned from. The events before it were compacted into a snapshot
    /// (ADR-0026), which keeps state and not events, so nothing is learned from them.
    pub from_seq: u64,
    pub learnings: Vec<Learning>,
}

/// The state of the log at `path` and its learnings, after checking it against `key`, both from one
/// read so they stand at the same head.
pub fn learnings(path: &Path, key: &PublicKey) -> Result<(State, Learnings), EngineError> {
    let contents = read_verified(path, key).map_err(EngineError::Verify)?;
    let state = state_from(&SCHEMA, contents.anchor.as_ref(), &contents.events)?;
    let from_seq = contents.anchor.as_ref().map_or(1, |a| a.seq + 1);
    Ok((state, learn(from_seq, &contents.events)?))
}

/// The learnings in `events`, which start at `from_seq`. An event this build cannot read is an
/// error, never skipped: a learning that left it out would say less happened than did.
pub fn learn(from_seq: u64, events: &[StoredEvent]) -> Result<Learnings, EngineError> {
    let mut learnings: Vec<Learning> = Vec::new();
    let mut aggregates = 0;
    for e in events {
        let event = Event::decode(&e.kind, e.schema_version, &e.payload)
            .map_err(|error| EngineError::Event { seq: e.seq, error })?;
        let (lesson, provenance) = match event {
            Event::JobEnded {
                outcome, reason, ..
            } => (Lesson::JobEnded { outcome, reason }, Provenance::Operator),
            Event::GateAnswered {
                gate,
                answer,
                reason,
                ..
            } if answer != Answer::Allow => (
                Lesson::GateAnswered {
                    gate,
                    answer,
                    reason,
                },
                Provenance::Operator,
            ),
            Event::HoldResolved { by, reason, .. } => {
                (Lesson::HoldResolved { by, reason }, Provenance::Operator)
            }
            Event::HoldRefused { reason, .. } => {
                (Lesson::HoldRefused { reason }, Provenance::Operator)
            }
            Event::TaskCancelled { by, reason, .. } => {
                (Lesson::TaskCancelled { by, reason }, Provenance::Operator)
            }
            Event::JobValues { job, values } => {
                (Lesson::Values { job, values }, Provenance::ModelFromTainted)
            }
            _ => continue,
        };
        let source = Source {
            seq: e.seq,
            hash: Digest::from_bytes(e.hash),
            origin: e.origin,
        };
        // A job returns its values once, so a values lesson is never grouped: only the others,
        // which are few (closed sets and reason codes), are searched.
        let grouped = !matches!(lesson, Lesson::Values { .. });
        match learnings
            .iter_mut()
            .take(if grouped { aggregates } else { 0 })
            .find(|l| l.lesson == lesson)
        {
            Some(learning) => {
                learning.count += 1;
                learning.first_at = learning.first_at.min(e.at);
                learning.last_at = learning.last_at.max(e.at);
                if learning.sources.len() == MAX_SOURCES {
                    learning.sources.remove(0);
                }
                learning.sources.push(source);
            }
            None => {
                let learning = Learning {
                    lesson,
                    provenance,
                    count: 1,
                    first_at: e.at,
                    last_at: e.at,
                    sources: vec![source],
                };
                // Aggregates are kept first, so the search above covers only them.
                if grouped {
                    learnings.insert(aggregates, learning);
                    aggregates += 1;
                } else {
                    learnings.push(learning);
                }
            }
        }
    }
    // The most recent first; two learned at the same time, the one whose latest event is later.
    learnings
        .sort_by_key(|l| std::cmp::Reverse((l.last_at, l.sources.last().map_or(0, |s| s.seq))));
    Ok(Learnings {
        from_seq,
        learnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyalos_types::event::SCHEMA_VERSION;
    use hyalos_types::{HoldId, Signed, TaskId};

    fn stored(seq: u64, origin: Origin, event: &Event) -> StoredEvent {
        StoredEvent {
            seq,
            at: Timestamp::from_millis(1_000 * seq),
            origin,
            kind: event.kind().to_string(),
            schema_version: SCHEMA_VERSION,
            payload: event.encode().unwrap(),
            hash: [seq as u8; 32],
        }
    }

    fn code(text: &str) -> ReasonCode {
        ReasonCode::new(text).unwrap()
    }

    fn ended(job: u64, outcome: JobOutcome, reason: &str) -> Event {
        Event::JobEnded {
            job: JobId::from_seq(job),
            outcome,
            reason: code(reason),
        }
    }

    #[test]
    fn learnings_group_by_lesson_with_provenance_and_their_source_events() {
        let gate = |answer, reason| Event::GateAnswered {
            job: JobId::from_seq(2),
            gate: Gate::Egress,
            inputs: Digest::from_bytes([7; 32]),
            answer,
            reason: code(reason),
        };
        let deny = gate(Answer::Deny, "egress.host_not_allowed");
        let allow = gate(Answer::Allow, "egress.allowed");
        let values = Event::JobValues {
            job: JobId::from_seq(2),
            values: vec![TypedValue::Choice(1), TypedValue::Bool(true)],
        };
        let events = [
            stored(
                1,
                Origin::Engine,
                &Event::TaskCreated {
                    request: Digest::from_bytes([1; 32]),
                },
            ),
            stored(3, Origin::Broker, &deny),
            stored(4, Origin::Broker, &allow),
            stored(
                5,
                Origin::Engine,
                &ended(2, JobOutcome::Failed, "harness.no_result"),
            ),
            stored(6, Origin::Broker, &deny),
            stored(7, Origin::Engine, &values),
            stored(
                8,
                Origin::Engine,
                &ended(9, JobOutcome::Failed, "harness.no_result"),
            ),
        ];
        let learned = learn(1, &events).unwrap();
        assert_eq!(learned.from_seq, 1);
        let lessons: Vec<_> = learned.learnings.iter().map(|l| &l.lesson).collect();
        assert_eq!(
            lessons,
            [
                &Lesson::JobEnded {
                    outcome: JobOutcome::Failed,
                    reason: code("harness.no_result")
                },
                &Lesson::Values {
                    job: JobId::from_seq(2),
                    values: vec![TypedValue::Choice(1), TypedValue::Bool(true)]
                },
                &Lesson::GateAnswered {
                    gate: Gate::Egress,
                    answer: Answer::Deny,
                    reason: code("egress.host_not_allowed")
                },
            ],
            "the most recent first, and an allow is not a lesson"
        );
        let failed = &learned.learnings[0];
        assert_eq!(
            (
                failed.count,
                failed.provenance,
                failed.first_at,
                failed.last_at
            ),
            (
                2,
                Provenance::Operator,
                Timestamp::from_millis(5_000),
                Timestamp::from_millis(8_000)
            )
        );
        let refused = &learned.learnings[2];
        assert_eq!(
            refused.sources,
            [3, 6].map(|seq| Source {
                seq,
                hash: Digest::from_bytes([seq as u8; 32]),
                origin: Origin::Broker
            })
        );
        assert_eq!(
            learned.learnings[1].provenance,
            Provenance::ModelFromTainted,
            "values a model chose are tainted unless something that knows says otherwise"
        );
    }

    #[test]
    fn a_learning_keeps_its_latest_sources_and_counts_them_all() {
        let events: Vec<_> = (1..=MAX_SOURCES as u64 + 5)
            .map(|seq| {
                stored(
                    seq,
                    Origin::Engine,
                    &ended(1, JobOutcome::Succeeded, "harness.submitted"),
                )
            })
            .collect();
        let learned = learn(1, &events).unwrap();
        let only = &learned.learnings[0];
        assert_eq!(only.count, MAX_SOURCES as u64 + 5);
        assert_eq!(only.sources.len(), MAX_SOURCES);
        assert_eq!(only.sources[0].seq, 6);
        assert_eq!(
            only.sources.last().map(|s| s.seq),
            Some(MAX_SOURCES as u64 + 5)
        );
    }

    #[test]
    fn decisions_are_learned_by_decider_and_reason() {
        let by = Resolution::Human;
        let events = [
            stored(
                1,
                Origin::Engine,
                &Event::TaskCancelled {
                    task: TaskId::from_seq(1),
                    by,
                    reason: code("operator.cancelled"),
                    signed: Signed::None,
                },
            ),
            stored(
                2,
                Origin::Engine,
                &Event::HoldRefused {
                    hold: HoldId::from_seq(1),
                    reason: code("egress.host_not_allowed"),
                },
            ),
        ];
        let learned = learn(1, &events).unwrap();
        assert_eq!(
            learned
                .learnings
                .iter()
                .map(|l| l.lesson.clone())
                .collect::<Vec<_>>(),
            [
                Lesson::HoldRefused {
                    reason: code("egress.host_not_allowed")
                },
                Lesson::TaskCancelled {
                    by,
                    reason: code("operator.cancelled")
                },
            ]
        );
    }

    #[test]
    fn an_event_that_cannot_be_read_is_an_error_not_a_gap() {
        let mut bad = stored(
            4,
            Origin::Engine,
            &ended(1, JobOutcome::Failed, "harness.no_result"),
        );
        bad.payload = b"{}".to_vec();
        assert!(matches!(
            learn(1, &[bad]),
            Err(EngineError::Event { seq: 4, .. })
        ));
    }
}
