//! Auditing snapshots against the archives they stand for (ADR-0026).
//!
//! `hyalos log verify` shows a snapshot is the engine's and unaltered. It does not show the state in it
//! is what the events replay to: the engine signed whatever it was holding. This replays the archives
//! and the live log from event 1, and at every snapshot compares the replayed state with the one the
//! snapshot carries. It reads only, and needs only the public key.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use hyalos_log::{Head, PublicKey, StoredEvent, VerifyError, read_verified, verify_history};
use hyalos_types::event::SCHEMA;
use hyalos_types::snapshot::StateImage;

use crate::engine::{EngineError, replay_from};
use crate::state::State;

/// Events that no archive given covers, and so the snapshots after them could not be audited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    /// The first event missing.
    pub from: u64,
    /// The last event of the first snapshot that could not be audited.
    pub through: u64,
}

/// What an audit found when nothing differed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audit {
    /// How many snapshots the live log holds.
    pub snapshots: usize,
    /// How many of them were compared with a replay.
    pub audited: usize,
    /// How many events were replayed: every event from 1 to the end of the history that was covered.
    pub replayed: u64,
    /// Set when the archives given did not reach the next snapshot.
    pub gap: Option<Gap>,
}

impl Audit {
    /// Whether every snapshot was audited. An audit that is not complete is not a pass.
    pub fn complete(&self) -> bool {
        self.audited == self.snapshots
    }
}

#[derive(Debug)]
pub enum AuditError {
    /// The log or an archive does not verify, or an archive is not part of this log's history.
    Verify(VerifyError),
    /// The events do not replay: the log is damaged or was written by a buggy engine.
    Replay(EngineError),
    /// A snapshot's state cannot be read.
    Snapshot { seq: u64, reason: String },
    /// A snapshot's state is not what the events replay to.
    Mismatch { seq: u64, difference: String },
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditError::Verify(e) => write!(f, "{e}"),
            AuditError::Replay(e) => write!(f, "the events do not replay: {e}"),
            AuditError::Snapshot { seq, reason } => {
                write!(f, "the snapshot at event {seq} cannot be read: {reason}")
            }
            AuditError::Mismatch { seq, difference } => write!(
                f,
                "the snapshot at event {seq} is not what the events replay to: {difference}"
            ),
        }
    }
}

impl std::error::Error for AuditError {}

/// Replays the history from event 1 through `archives` and the live log at `live`, and compares the
/// state at each snapshot with the state the snapshot carries. It stops at the first that differs and
/// names the first field. Archives that do not reach as far as the next snapshot leave that snapshot
/// and the ones after it unaudited, which the result says; it is not a pass.
pub fn audit(
    live: &Path,
    archives: &[PathBuf],
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<Audit, AuditError> {
    // Each file must verify and each archive must end at one of this log's snapshots.
    verify_history(live, archives, key, expect_head).map_err(AuditError::Verify)?;
    let live_log = read_verified(live, key).map_err(AuditError::Verify)?;

    let mut events: BTreeMap<u64, StoredEvent> = BTreeMap::new();
    // An event two files share is the same event: the hash chain covers its stored bytes, and each
    // archive was checked to end at a snapshot of this log. So the first copy is as good as the second,
    // and an archive given twice changes nothing.
    for path in archives {
        let archive = read_verified(path, key).map_err(|error| {
            AuditError::Verify(VerifyError::Archive {
                path: path.clone(),
                error: Box::new(error),
            })
        })?;
        for e in archive.events {
            events.entry(e.seq).or_insert(e);
        }
    }
    for e in live_log.events {
        events.entry(e.seq).or_insert(e);
    }

    // Only the run of events that starts at 1 and has no gap can be replayed.
    let mut covered: Vec<StoredEvent> = Vec::new();
    for (seq, event) in events {
        if seq != covered.len() as u64 + 1 {
            break;
        }
        covered.push(event);
    }
    let covered_to = covered.len() as u64;

    let mut state = State::default();
    let (mut done, mut audited) = (0usize, 0usize);
    for anchor in &live_log.anchors {
        if anchor.seq > covered_to {
            break;
        }
        let upto = anchor.seq as usize;
        state = replay_from(&SCHEMA, state, &covered[done..upto]).map_err(AuditError::Replay)?;
        done = upto;
        let claimed = StateImage::decode(anchor.state_version, &anchor.state).map_err(|e| {
            AuditError::Snapshot {
                seq: anchor.seq,
                reason: e.to_string(),
            }
        })?;
        if claimed.last_seq != anchor.seq {
            return Err(AuditError::Snapshot {
                seq: anchor.seq,
                reason: format!("its state ends at event {}", claimed.last_seq),
            });
        }
        let replayed = state
            .to_image()
            .map_err(|job| AuditError::Mismatch {
                seq: anchor.seq,
                difference: format!("the replay has job {job}'s unknown answer with no hold yet"),
            })?
            .as_kept_at(anchor.state_version);
        if let Some(difference) = replayed.first_difference(&claimed) {
            return Err(AuditError::Mismatch {
                seq: anchor.seq,
                difference,
            });
        }
        audited += 1;
    }
    // The events after the last snapshot checked must replay too, so the whole history is accounted for.
    replay_from(&SCHEMA, state, &covered[done..]).map_err(AuditError::Replay)?;

    let gap = live_log.anchors.get(audited).map(|next| Gap {
        from: covered_to + 1,
        through: next.seq,
    });
    Ok(Audit {
        snapshots: live_log.anchors.len(),
        audited,
        replayed: covered_to,
        gap,
    })
}
