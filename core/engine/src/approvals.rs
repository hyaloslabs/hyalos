//! Checking the operator's signature on every human decision in a log (ADR-0003, ADR-0070).
//!
//! The engine signs every event, so its key alone could write a human decision. This check needs
//! only the two public keys, `engine.pub` and `operator.pub`, and never takes either from the log:
//! each human decision written at schema version 13 or later must carry the operator's signature
//! over the event, this log's engine key and the event the operator saw, that event must be the one
//! in this log at that place, and it must come no earlier than the previous signed decision. A human
//! decision from before version 13 carried none; it passes and is listed, so the operator knows which
//! decisions rest on the engine's key alone.

use std::fmt;
use std::path::Path;

use hyalos_log::{PublicKey, VerifyError, read_verified};
use hyalos_types::event::SCHEMA;
use hyalos_types::snapshot::StateImage;
use hyalos_types::{Event, EventError, OperatorPublic, Resolution, Signed};

/// The first schema version that signs human decisions.
pub const SIGNED_SINCE: u32 = 13;

/// The kinds that record a decision.
const DECISIONS: [&str; 6] = [
    "hold.resolved",
    "task.cancelled",
    "policy.approved",
    "handle.released",
    "agent.issued",
    "agent.revoked",
];

/// The human decisions in a log that passed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Approvals {
    /// The events the operator signed.
    pub signed: Vec<u64>,
    /// Human decisions written before schema version 13, which carry no signature: they rest on the
    /// engine's key alone.
    pub unrecorded: Vec<Unrecorded>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrecorded {
    pub seq: u64,
    pub kind: String,
}

#[derive(Debug)]
pub enum ApprovalError {
    Verify(VerifyError),
    Event {
        seq: u64,
        error: EventError,
    },
    /// The latest snapshot could not be read.
    Snapshot(String),
    /// A human decision written at version 13 or later carries no operator signature.
    Unsigned {
        seq: u64,
    },
    /// The log has signed decisions and no operator key was given to check them with.
    NoOperatorKey {
        seq: u64,
    },
    /// Signed by a key that is not the operator's.
    WrongOperator {
        seq: u64,
    },
    /// The signature does not hold for this event in this log.
    BadSignature {
        seq: u64,
    },
    /// The event the approval names is not the one in this log at that place, or comes before the
    /// previous signed decision or not before the decision itself.
    WrongPlace {
        seq: u64,
        after: u64,
    },
}

impl fmt::Display for ApprovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApprovalError::Verify(e) => write!(f, "the event log failed verification: {e}"),
            ApprovalError::Event { seq, error } => write!(f, "event {seq}: {error}"),
            ApprovalError::Snapshot(e) => write!(f, "the latest snapshot could not be read: {e}"),
            ApprovalError::Unsigned { seq } => write!(
                f,
                "event {seq} is a human decision without the operator's signature"
            ),
            ApprovalError::NoOperatorKey { seq } => write!(
                f,
                "event {seq} is signed by the operator, and there is no operator public key to \
                 check it with"
            ),
            ApprovalError::WrongOperator { seq } => {
                write!(
                    f,
                    "event {seq} is signed by a key that is not the operator's"
                )
            }
            ApprovalError::BadSignature { seq } => write!(
                f,
                "the operator's signature on event {seq} does not hold for it in this log"
            ),
            ApprovalError::WrongPlace { seq, after } => write!(
                f,
                "event {seq} was signed after event {after}, which is not that event in this log \
                 or not between the previous signed decision and this one"
            ),
        }
    }
}

impl std::error::Error for ApprovalError {}

/// Checks every human decision in the log at `path`, after checking the log against the engine's
/// public key `engine`. `operator` is the operator's public key from `operator.pub`; with none, a log
/// with no signed decision still passes. It writes nothing.
pub fn approvals(
    path: &Path,
    engine: &PublicKey,
    operator: Option<&OperatorPublic>,
) -> Result<Approvals, ApprovalError> {
    let contents = read_verified(path, engine).map_err(ApprovalError::Verify)?;
    let mut last_signed = match &contents.anchor {
        Some(anchor) => {
            StateImage::decode(anchor.state_version, &anchor.state)
                .map_err(|e| ApprovalError::Snapshot(e.to_string()))?
                .last_approval
        }
        None => None,
    };
    let mut out = Approvals::default();
    for e in contents
        .events
        .iter()
        .filter(|e| DECISIONS.contains(&e.kind.as_str()))
    {
        let seq = e.seq;
        let event = Event::decode_with(&SCHEMA, &e.kind, e.schema_version, &e.payload)
            .map_err(|error| ApprovalError::Event { seq, error })?;
        if event.decided_by() != Some(Resolution::Human) {
            continue;
        }
        let approval = match event.signed() {
            Some(Signed::Operator(approval)) => approval,
            Some(Signed::Unrecorded) if e.schema_version < SIGNED_SINCE => {
                out.unrecorded.push(Unrecorded {
                    seq,
                    kind: e.kind.clone(),
                });
                continue;
            }
            _ => return Err(ApprovalError::Unsigned { seq }),
        };
        let operator = operator.ok_or(ApprovalError::NoOperatorKey { seq })?;
        if approval.operator != *operator {
            return Err(ApprovalError::WrongOperator { seq });
        }
        let bytes = event
            .approval_bytes()
            .map_err(|error| ApprovalError::Event { seq, error })?;
        if !approval.is_by(operator, engine.as_bytes(), &bytes) {
            return Err(ApprovalError::BadSignature { seq });
        }
        let after = approval.after;
        let wrong = ApprovalError::WrongPlace { seq, after };
        if after >= seq || last_signed.is_some_and(|last| after < last) {
            return Err(wrong);
        }
        // The event named must be this log's. One before the latest snapshot is no longer in the
        // file, and that history rests on the engine's signature on the snapshot (ADR-0026).
        let seen = if after == 0 {
            Some([0; 32])
        } else {
            contents
                .events
                .iter()
                .find(|x| x.seq == after)
                .map(|x| x.hash)
        };
        if seen.is_some_and(|hash| hash != *approval.after_hash.as_bytes())
            || (seen.is_none() && contents.anchor.is_none())
        {
            return Err(wrong);
        }
        last_signed = Some(seq);
        out.signed.push(seq);
    }
    Ok(out)
}
