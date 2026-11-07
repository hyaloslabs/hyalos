//! Checking the operator's signature on every human decision in a log (ADR-0003, ADR-0070).
//!
//! The engine signs every event, so its key alone could write a human decision. This check needs
//! only the two public keys, `engine.pub` and `operator.pub`, and never takes either from the log:
//! each human decision written at schema version 13 or later must carry the operator's signature
//! over the event, this log's engine key and the event the operator saw, that event must be the one
//! in this log at that place, and it must come no earlier than the previous signed decision. A human
//! decision from before version 13 carried none; it passes and is listed, so the operator knows which
//! decisions rest on the engine's key alone.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use hyalos_log::{Anchor, PublicKey, StoredEvent, VerifyError, read_verified};
use hyalos_types::event::SCHEMA;
use hyalos_types::snapshot::StateImage;
use hyalos_types::{Event, EventError, OperatorPublic, Resolution, SIGNED_SINCE, Signed};

/// The kinds that record a decision.
const DECISIONS: [&str; 7] = [
    "hold.resolved",
    "task.cancelled",
    "policy.approved",
    "handle.released",
    "agent.issued",
    "agent.revoked",
    "merge.approved",
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

/// Checks every human decision in the log at `live` and in the `archives` given, each checked against
/// the engine's public key `engine`, in sequence order across the files. `operator` is the
/// operator's public key from `operator.pub`; with none, a log with no signed decision still passes.
/// History that is in no file given, before the earliest snapshot reached, rests on the engine's
/// signature on that snapshot (ADR-0026); give the archives to check it too. It writes nothing.
pub fn approvals(
    live: &Path,
    archives: &[PathBuf],
    engine: &PublicKey,
    operator: Option<&OperatorPublic>,
) -> Result<Approvals, ApprovalError> {
    let mut files = Vec::new();
    for path in archives.iter().map(PathBuf::as_path).chain([live]) {
        files.push(read_verified(path, engine).map_err(ApprovalError::Verify)?);
    }
    let mut events: BTreeMap<u64, &StoredEvent> = BTreeMap::new();
    let mut anchors: BTreeMap<u64, &Anchor> = BTreeMap::new();
    for file in &files {
        events.extend(file.events.iter().map(|e| (e.seq, e)));
        anchors.extend(file.anchors.iter().map(|a| (a.seq, a)));
    }
    let mut last_signed = None;
    let mut signed_era = false;
    let mut previous: Option<u64> = None;
    let mut out = Approvals::default();
    for (&seq, e) in &events {
        // At the start, and after a stretch no file given holds, carry on from the snapshot there.
        if previous.is_none_or(|p| seq != p + 1) {
            let image = match anchors.get(&(seq - 1)) {
                Some(anchor) => Some(
                    StateImage::decode(anchor.state_version, &anchor.state)
                        .map_err(|e| ApprovalError::Snapshot(e.to_string()))?,
                ),
                None => None,
            };
            // Never forgets what an earlier file showed: a gap with no snapshot cannot reopen the
            // unsigned era or let an approval go back past one already checked.
            last_signed = last_signed.max(image.as_ref().and_then(|i| i.last_approval));
            signed_era |= image.is_some_and(|i| i.signed_since.is_some());
        }
        previous = Some(seq);
        let decision = DECISIONS.contains(&e.kind.as_str());
        let written_signed = e.schema_version >= SIGNED_SINCE;
        let unsigned_allowed = !written_signed && !signed_era;
        signed_era |= written_signed;
        if !decision {
            continue;
        }
        let event = Event::decode_with(&SCHEMA, &e.kind, e.schema_version, &e.payload)
            .map_err(|error| ApprovalError::Event { seq, error })?;
        if event.decided_by() != Some(Resolution::Human) {
            continue;
        }
        let approval = match event.signed() {
            Some(Signed::Operator(approval)) => approval,
            // The version is the writer's to choose: once the log has reached the signed version,
            // an older one does not excuse a missing signature.
            Some(Signed::Unrecorded) if unsigned_allowed => {
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
        // The event named must be this log's. One in no file given lies before a snapshot, and
        // rests on the engine's signature on it (ADR-0026).
        let seen = if after == 0 {
            Some([0; 32])
        } else {
            events.get(&after).map(|x| x.hash)
        };
        let before_a_snapshot = anchors.keys().any(|&a| a >= after);
        if seen.is_some_and(|hash| hash != *approval.after_hash.as_bytes())
            || (seen.is_none() && !before_a_snapshot)
        {
            return Err(wrong);
        }
        last_signed = Some(seq);
        out.signed.push(seq);
    }
    Ok(out)
}
