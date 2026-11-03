//! Compacting the log: archive the events, then replace them with a signed snapshot (ADR-0026).
//!
//! Two steps, so the order that keeps history safe is the only order there is:
//!
//! 1. [`Log::archive`] writes a verified copy of the whole log to a new file. Nothing in the live log
//!    changes.
//! 2. [`Log::compact`], given the head that was archived, removes the events from the live log and
//!    writes the snapshot in one transaction. If the log moved on in between, it refuses and changes
//!    nothing.
//!
//! The events are never rewritten and the chain is never re-hashed: events after the snapshot keep the
//! hashes and signatures they were written with, and the first of them links to the snapshot's head
//! hash, which is the hash of the last event removed. The snapshot is the one thing added.

use std::convert::Infallible;
use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use hyalos_types::Timestamp;
use rusqlite::TransactionBehavior;

use crate::copy::{CopyError, copy_connection};
use crate::format::{self, AnchorFields};
use crate::store::{
    Anchor, FORMAT_VERSION, Head, Log, LogError, NO_DELETE_EVENTS, StoredEvent, read_events,
    read_head, read_latest_anchor,
};

/// Why a log was not compacted. Whatever the reason, the live log is as it was.
#[derive(Debug)]
pub enum CompactError<E = Infallible> {
    /// There are no events since the last snapshot, so there is nothing to replace.
    NothingToCompact,
    /// The archive could not be written or did not verify. The live log was not touched.
    Archive(CopyError),
    /// The log has events the archive does not. Archive again.
    Moved {
        archived: Head,
        now: Option<Head>,
    },
    /// The caller could not produce the state to snapshot.
    Snapshot(E),
    Log(LogError),
}

impl<E> CompactError<E> {
    /// Changes the type of the snapshot error, so the error of [`Log::archive`], which has none, can be
    /// carried with the error of [`Log::compact`].
    pub fn map_snapshot<F>(self, f: impl FnOnce(E) -> F) -> CompactError<F> {
        match self {
            CompactError::NothingToCompact => CompactError::NothingToCompact,
            CompactError::Archive(e) => CompactError::Archive(e),
            CompactError::Moved { archived, now } => CompactError::Moved { archived, now },
            CompactError::Snapshot(e) => CompactError::Snapshot(f(e)),
            CompactError::Log(e) => CompactError::Log(e),
        }
    }
}

impl<E: fmt::Display> fmt::Display for CompactError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompactError::NothingToCompact => {
                write!(f, "there are no events since the last snapshot to compact")
            }
            CompactError::Archive(e) => write!(f, "the archive could not be made: {e}"),
            CompactError::Moved { archived, now } => match now {
                Some(now) => write!(
                    f,
                    "the log changed while it was being archived (archived {archived}, now {now}); nothing was compacted, so archive again"
                ),
                None => write!(f, "the log is now empty; nothing was compacted"),
            },
            CompactError::Snapshot(e) => write!(f, "the state could not be snapshotted: {e}"),
            CompactError::Log(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CompactError<E> {}

impl<E> From<LogError> for CompactError<E> {
    fn from(e: LogError) -> Self {
        CompactError::Log(e)
    }
}

impl<E> From<rusqlite::Error> for CompactError<E> {
    fn from(e: rusqlite::Error) -> Self {
        CompactError::Log(e.into())
    }
}

impl Log {
    /// Writes a copy of the log to the new file `dst` and returns the head it holds. The copy is taken
    /// by SQLite and verified before it exists under that name, exactly as a backup is (ADR-0021); it
    /// includes every earlier snapshot, so it can be checked on its own. Nothing is overwritten.
    pub fn archive(&self, dst: &Path) -> Result<Head, CompactError> {
        let waiting: i64 = self
            .conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
        if waiting == 0 {
            return Err(CompactError::NothingToCompact);
        }
        let verified = copy_connection(&self.conn, dst, &self.key.public(), None)
            .map_err(CompactError::Archive)?;
        // An archive is evidence, so it is made read-only: an accident cannot edit it, and removing it
        // takes a deliberate act. (Someone with the file can still change the mode; the chain and the
        // signatures are what catch that.)
        std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o400))
            .map_err(|e| CompactError::Archive(CopyError::from(e)))?;
        verified.head.ok_or(CompactError::NothingToCompact)
    }

    /// Replaces every event since the last snapshot with a new snapshot, in one transaction.
    ///
    /// `archived` is the head [`Log::archive`] returned: if the log has moved since, nothing happens.
    /// `snapshot` is given the previous snapshot (if any) and the events about to be removed, and
    /// returns the state version and bytes to keep. The log signs and stores them without reading them.
    pub fn compact<E>(
        &mut self,
        archived: Head,
        at: Timestamp,
        snapshot: impl FnOnce(Option<&Anchor>, &[StoredEvent]) -> Result<(u32, Vec<u8>), E>,
    ) -> Result<Anchor, CompactError<E>> {
        let public = self.key.public();
        let at_millis = at.as_millis();
        let at_stored = i64::try_from(at_millis).map_err(|_| LogError::OutOfRange("timestamp"))?;
        let seq_stored =
            i64::try_from(archived.seq).map_err(|_| LogError::OutOfRange("sequence number"))?;

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = read_head(&tx)?;
        if now != Some(archived) {
            return Err(CompactError::Moved { archived, now });
        }
        let prior = read_latest_anchor(&tx)?;
        let events = read_events(&tx)?;
        if events.is_empty() {
            return Err(CompactError::NothingToCompact);
        }
        let (state_version, state) =
            snapshot(prior.as_ref(), &events).map_err(CompactError::Snapshot)?;

        let digest = format::state_digest(&state);
        let prev_anchor = prior
            .as_ref()
            .map_or_else(|| format::first_anchor_prev(public.as_bytes()), |a| a.hash);
        let hash = format::anchor_hash(&AnchorFields {
            seq: archived.seq,
            at_millis,
            head_hash: &archived.hash,
            state_version,
            state_digest: &digest,
            prev_anchor: &prev_anchor,
        });
        let signature = self.key.sign_anchor(&hash);

        tx.execute(
            "INSERT INTO anchors (seq, at, head_hash, state_version, state, state_digest, prev_anchor, hash, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                seq_stored,
                at_stored,
                archived.hash.as_slice(),
                state_version,
                state,
                digest.as_slice(),
                prev_anchor.as_slice(),
                hash.as_slice(),
                signature.as_slice(),
            ],
        )?;
        // The guard against deleting events is lifted for this one statement and put back before the
        // transaction ends. If anything fails, the rollback restores the guard along with the events.
        tx.execute_batch("DROP TRIGGER events_no_delete; DELETE FROM events;")?;
        tx.execute_batch(NO_DELETE_EVENTS)?;
        tx.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'format_version'",
            [FORMAT_VERSION],
        )?;
        tx.commit()?;

        // Give the space back. This is only housekeeping: the compaction has happened, and a failure
        // here changes nothing about what the log holds.
        let _ = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");

        Ok(Anchor {
            seq: archived.seq,
            at,
            head_hash: archived.hash,
            state_version,
            state,
            hash,
        })
    }
}
