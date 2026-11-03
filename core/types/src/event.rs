//! The events that tasks, jobs and holds are rebuilt from (ADR-0003, ADR-0009, ADR-0019).
//!
//! Events hold typed values, digests and references, never credentials, tokens or raw text. Each one is
//! stored as JSON with a `kind` tag that repeats the log's `kind` column. Decoding refuses unknown
//! fields, an unknown kind and an unknown schema version, so a payload the engine does not understand
//! is an error and never a guess.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Answer, Digest, HoldId, JobId, ReasonCode, TaskId};

/// The version these events are written with. Reading older versions (upcasting) is S1.4.
pub const SCHEMA_VERSION: u32 = 1;

/// How a job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOutcome {
    Succeeded,
    Failed,
    /// The manifest's deadline passed (ADR-0004).
    DeadlineExceeded,
    /// The job was stopped because its task is being held (ADR-0009).
    Held,
}

/// The gates that answer per request (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    Egress,
    Content,
    Role,
    Manifest,
}

/// Why a hold was opened. A gate that answers `deny` does not hold; it refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// The gate answered unknown.
    Unknown,
    /// The gate did not answer in time, which counts as unknown.
    TimedOut,
}

/// How a hold was resolved: the tiers of ADR-0012, or the gate answering on re-evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Reevaluated,
    Policy,
    Verifier,
    Human,
}

/// One state change. Identifiers of new things are not in the event: they are the sequence number it
/// is appended at ([`crate::ids`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Event {
    /// A task was asked for. `request` is the digest of the request document, which is held elsewhere.
    #[serde(rename = "task.created")]
    TaskCreated { request: Digest },
    #[serde(rename = "task.completed")]
    TaskCompleted { task: TaskId },
    #[serde(rename = "job.started")]
    JobStarted { task: TaskId },
    #[serde(rename = "job.ended")]
    JobEnded { job: JobId, outcome: JobOutcome },
    /// A gate answered a request made by a running job. `inputs` is the digest of what it was asked.
    /// An `unknown` answer is always followed, in the same write, by the job ending held and the hold
    /// opening (ADR-0020).
    #[serde(rename = "gate.answered")]
    GateAnswered {
        job: JobId,
        gate: Gate,
        inputs: Digest,
        answer: Answer,
        reason: ReasonCode,
    },
    /// A gate answered unknown or timed out. `inputs` is the digest of what the gate was asked.
    #[serde(rename = "hold.opened")]
    HoldOpened {
        task: TaskId,
        gate: Gate,
        inputs: Digest,
        reason: HoldReason,
    },
    #[serde(rename = "hold.resolved")]
    HoldResolved { hold: HoldId, by: Resolution },
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventError {
    Encode(String),
    Decode(String),
    UnsupportedVersion(u32),
    /// The log's `kind` column and the payload's own `kind` disagree.
    KindMismatch {
        column: String,
        payload: &'static str,
    },
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventError::Encode(e) => write!(f, "cannot encode the event: {e}"),
            EventError::Decode(e) => write!(f, "cannot decode the event: {e}"),
            EventError::UnsupportedVersion(v) => {
                write!(f, "event schema version {v} is not supported by this build")
            }
            EventError::KindMismatch { column, payload } => write!(
                f,
                "the log says this is a `{column}` event but its payload is a `{payload}` event"
            ),
        }
    }
}

impl std::error::Error for EventError {}

impl Event {
    /// The value for the log's `kind` column.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::TaskCreated { .. } => "task.created",
            Event::TaskCompleted { .. } => "task.completed",
            Event::JobStarted { .. } => "job.started",
            Event::JobEnded { .. } => "job.ended",
            Event::GateAnswered { .. } => "gate.answered",
            Event::HoldOpened { .. } => "hold.opened",
            Event::HoldResolved { .. } => "hold.resolved",
        }
    }

    /// The bytes stored in the log's `payload` column.
    pub fn encode(&self) -> Result<Vec<u8>, EventError> {
        serde_json::to_vec(self).map_err(|e| EventError::Encode(e.to_string()))
    }

    /// Reads an event back from a log row's `kind`, `schema_version` and `payload`.
    pub fn decode(kind: &str, schema_version: u32, payload: &[u8]) -> Result<Self, EventError> {
        if schema_version != SCHEMA_VERSION {
            return Err(EventError::UnsupportedVersion(schema_version));
        }
        let event: Event =
            serde_json::from_slice(payload).map_err(|e| EventError::Decode(e.to_string()))?;
        if event.kind() != kind {
            return Err(EventError::KindMismatch {
                column: kind.to_string(),
                payload: event.kind(),
            });
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(byte: u8) -> Digest {
        Digest::from_bytes([byte; 32])
    }

    fn samples() -> Vec<(Event, String)> {
        let h = |b: u8| format!("{b:02x}").repeat(32);
        vec![
            (
                Event::TaskCreated { request: d(1) },
                format!(r#"{{"kind":"task.created","request":"{}"}}"#, h(1)),
            ),
            (
                Event::TaskCompleted {
                    task: TaskId::from_seq(3),
                },
                r#"{"kind":"task.completed","task":3}"#.to_string(),
            ),
            (
                Event::JobStarted {
                    task: TaskId::from_seq(3),
                },
                r#"{"kind":"job.started","task":3}"#.to_string(),
            ),
            (
                Event::JobEnded {
                    job: JobId::from_seq(4),
                    outcome: JobOutcome::DeadlineExceeded,
                },
                r#"{"kind":"job.ended","job":4,"outcome":"deadline_exceeded"}"#.to_string(),
            ),
            (
                Event::GateAnswered {
                    job: JobId::from_seq(4),
                    gate: Gate::Content,
                    inputs: d(6),
                    answer: Answer::Deny,
                    reason: ReasonCode::new("content.secret_pattern").unwrap(),
                },
                format!(
                    r#"{{"kind":"gate.answered","job":4,"gate":"content","inputs":"{}","answer":"deny","reason":"content.secret_pattern"}}"#,
                    h(6)
                ),
            ),
            (
                Event::HoldOpened {
                    task: TaskId::from_seq(3),
                    gate: Gate::Egress,
                    inputs: d(2),
                    reason: HoldReason::TimedOut,
                },
                format!(
                    r#"{{"kind":"hold.opened","task":3,"gate":"egress","inputs":"{}","reason":"timed_out"}}"#,
                    h(2)
                ),
            ),
            (
                Event::HoldResolved {
                    hold: HoldId::from_seq(5),
                    by: Resolution::Human,
                },
                r#"{"kind":"hold.resolved","hold":5,"by":"human"}"#.to_string(),
            ),
        ]
    }

    #[test]
    fn every_event_has_a_pinned_encoding_and_round_trips() {
        // A change to these strings changes what is stored in every log, so it must be deliberate.
        for (event, json) in samples() {
            assert_eq!(String::from_utf8(event.encode().unwrap()).unwrap(), json);
            assert_eq!(
                Event::decode(event.kind(), SCHEMA_VERSION, json.as_bytes()).unwrap(),
                event
            );
        }
    }

    #[test]
    fn the_kind_column_matches_the_payload_tag() {
        for (event, json) in samples() {
            assert!(json.contains(&format!("\"kind\":\"{}\"", event.kind())));
        }
    }

    #[test]
    fn a_wrong_kind_column_is_refused() {
        let json = r#"{"kind":"job.started","task":3}"#;
        let err = Event::decode("task.completed", SCHEMA_VERSION, json.as_bytes()).unwrap_err();
        assert!(matches!(err, EventError::KindMismatch { .. }), "{err}");
    }

    #[test]
    fn a_malformed_gate_answer_is_a_decode_error_not_just_a_kind_mismatch() {
        let zeros = "00".repeat(32);
        let good = format!(
            r#"{{"kind":"gate.answered","job":4,"gate":"egress","inputs":"{zeros}","answer":"allow","reason":"ok"}}"#
        );
        assert!(Event::decode("gate.answered", SCHEMA_VERSION, good.as_bytes()).is_ok());
        let bad = [
            good.replace(&zeros, "00"),
            good.replace("\"ok\"", "\"Free Text Here\""),
            good.replace("\"ok\"", "\"\""),
            good.replace("\"allow\"", "\"perhaps\""),
            good.replace("\"allow\"", "null"),
            good.replace(",\"reason\":\"ok\"", ""),
            good.replace("}", ",\"note\":\"x\"}"),
            good.replace("\"job\":4", "\"job\":\"4\""),
        ];
        for json in bad {
            let err = Event::decode("gate.answered", SCHEMA_VERSION, json.as_bytes()).unwrap_err();
            assert!(matches!(err, EventError::Decode(_)), "{json}: {err}");
        }
    }

    #[test]
    fn unknown_fields_kinds_and_versions_are_refused() {
        let bad = [
            r#"{"kind":"job.started","task":3,"extra":1}"#,
            r#"{"kind":"job.vanished","task":3}"#,
            r#"{"kind":"job.started"}"#,
            r#"{"kind":"job.started","task":"3"}"#,
            r#"{"kind":"job.ended","job":4,"outcome":"exploded"}"#,
            r#"{"task":3}"#,
            "not json",
            "",
        ];
        for json in bad {
            assert!(
                Event::decode("job.started", SCHEMA_VERSION, json.as_bytes()).is_err(),
                "{json}"
            );
        }
        let ok = r#"{"kind":"job.started","task":3}"#;
        assert_eq!(
            Event::decode("job.started", 2, ok.as_bytes()),
            Err(EventError::UnsupportedVersion(2))
        );
    }
}
