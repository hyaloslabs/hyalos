//! Checking a log from the outside (ADR-0018): `hyalos log verify`.
//!
//! The verifier trusts nothing in the file. It is given the public key and, optionally, the head the
//! operator recorded earlier, and it opens the database read-only.

use std::fmt;
use std::path::{Path, PathBuf};

use hyalos_types::Origin;
use rusqlite::{Connection, OpenFlags};

use crate::format::{self, Fields, Hash};
use crate::key::PublicKey;
use crate::store::Head;

/// Why one event failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Events were removed, inserted or reordered before this one.
    OutOfSequence { expected: u64 },
    /// Its `prev_hash` is not the previous event's hash. For event 1 it means a different key.
    ChainBroken,
    /// Its stored hash is not the hash of its contents.
    HashMismatch,
    /// Its signature does not verify with the supplied public key.
    BadSignature,
    /// A column is missing, has the wrong type or the wrong length.
    Malformed(&'static str),
    /// Snapshots must come in increasing order of the event they stand for.
    OutOfOrder,
    /// The state a snapshot carries is not the state its signed digest commits to.
    StateDigestMismatch,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::OutOfSequence { expected } => write!(
                f,
                "expected event {expected} here, so events were removed, inserted or reordered"
            ),
            Reason::ChainBroken => write!(
                f,
                "its prev_hash is not the hash of the event before it (for event 1: a different key)"
            ),
            Reason::HashMismatch => write!(f, "its stored hash does not match its contents"),
            Reason::BadSignature => write!(f, "its signature does not verify with the public key"),
            Reason::Malformed(column) => write!(f, "column `{column}` is unreadable"),
            Reason::OutOfOrder => write!(f, "snapshots are not in increasing order"),
            Reason::StateDigestMismatch => {
                write!(f, "its state is not the state its signature commits to")
            }
        }
    }
}

#[derive(Debug)]
pub enum VerifyError {
    /// The first event that fails, by sequence number.
    Event { seq: u64, reason: Reason },
    /// The first snapshot that fails, by the sequence number of the event it stands for the end of.
    Anchor { seq: u64, reason: Reason },
    /// An archive file does not verify.
    Archive {
        path: PathBuf,
        error: Box<VerifyError>,
    },
    /// An archive verifies but is not the history of any snapshot in this log: it ends somewhere no
    /// snapshot stands, or with a different hash than the snapshot there.
    ArchiveNotAnchored { path: PathBuf, head: Option<Head> },
    /// Every event verifies, but the log ends somewhere other than where the operator recorded.
    HeadMismatch { expected: Head, found: Option<Head> },
    /// The file could not be opened or read as an event log.
    Storage(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Event { seq, reason } => write!(f, "event {seq}: {reason}"),
            VerifyError::Anchor { seq, reason } => write!(f, "snapshot at event {seq}: {reason}"),
            VerifyError::Archive { path, error } => {
                write!(f, "archive {}: {error}", path.display())
            }
            VerifyError::ArchiveNotAnchored { path, head } => match head {
                Some(head) => write!(
                    f,
                    "archive {} ends at {head}, which is not where any snapshot of this log stands, so it is not part of this log's history",
                    path.display()
                ),
                None => write!(f, "archive {} holds no events", path.display()),
            },
            VerifyError::HeadMismatch { expected, found } => match found {
                Some(found) => write!(
                    f,
                    "the log ends at {found} but the expected head is {expected}: events may have been removed from the end, or this is a different log"
                ),
                None => write!(f, "the log is empty but the expected head is {expected}"),
            },
            VerifyError::Storage(e) => write!(f, "cannot read the event log: {e}"),
        }
    }
}

impl std::error::Error for VerifyError {}

impl From<rusqlite::Error> for VerifyError {
    fn from(e: rusqlite::Error) -> Self {
        VerifyError::Storage(e.to_string())
    }
}

/// A log that verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    /// How many events this file holds and checked. After compaction that is the events since the
    /// latest snapshot, not the whole history.
    pub events: u64,
    /// Where the chain ends: the last event, or the latest snapshot if no event follows it.
    pub head: Option<Head>,
    /// The latest snapshot, if events were compacted. The history up to here is vouched for by the
    /// engine's signature on it, not re-checked, unless its archives are checked too ([`verify_history`]).
    pub anchored: Option<Head>,
}

/// Verifies every event in the log at `path` against `key`. With `expect_head`, also requires the log
/// to end exactly there, which is the only way to notice events removed from the end.
pub fn verify(
    path: &Path,
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<Verified, VerifyError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    verify_connection(&conn, key, expect_head)
}

/// [`verify`] over a connection that is already open, so the file checked is the one in hand and not
/// whatever is at a path a moment later.
pub(crate) fn verify_connection(
    conn: &Connection,
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<Verified, VerifyError> {
    verify_detailed(conn, key, expect_head).map(|d| d.verified)
}

/// What a verification learned besides "it verified", for checking archives against snapshots.
pub(crate) struct Detail {
    pub verified: Verified,
    /// Every snapshot, in order.
    pub anchors: Vec<Head>,
    /// The sequence number of the first event this file should hold.
    pub starts_at: u64,
}

fn has_table(conn: &Connection, name: &str) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

/// Checks the chain of snapshots and returns where each stands. A log written before compaction
/// existed has no such table, which is the same as having no snapshots.
fn verify_anchors(conn: &Connection, key: &PublicKey) -> Result<Vec<Head>, VerifyError> {
    if !has_table(conn, "anchors")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT seq, at, head_hash, state_version, state, state_digest, prev_anchor, hash, signature
         FROM anchors ORDER BY seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut anchors = Vec::new();
    let mut prev_seq = 0u64;
    let mut prev = format::first_anchor_prev(key.as_bytes());
    while let Some(row) = rows.next()? {
        let seq = row
            .get::<_, i64>(0)
            .ok()
            .and_then(|s| u64::try_from(s).ok())
            .unwrap_or(0);
        let bad = |reason| VerifyError::Anchor { seq, reason };
        let malformed = |column| move |_| bad(Reason::Malformed(column));
        let hash_of = |column: &'static str, bytes: Vec<u8>| {
            Hash::try_from(bytes.as_slice()).map_err(|_| bad(Reason::Malformed(column)))
        };

        let at: i64 = row.get(1).map_err(malformed("at"))?;
        let head_hash = hash_of("head_hash", row.get(2).map_err(malformed("head_hash"))?)?;
        let state_version: u32 = row.get(3).map_err(malformed("state_version"))?;
        let state: Vec<u8> = row.get(4).map_err(malformed("state"))?;
        let stored_digest = hash_of(
            "state_digest",
            row.get(5).map_err(malformed("state_digest"))?,
        )?;
        let stored_prev = hash_of("prev_anchor", row.get(6).map_err(malformed("prev_anchor"))?)?;
        let stored_hash = hash_of("hash", row.get(7).map_err(malformed("hash"))?)?;
        let signature: Vec<u8> = row.get(8).map_err(malformed("signature"))?;
        let at_millis = u64::try_from(at).map_err(|_| bad(Reason::Malformed("at")))?;

        if seq <= prev_seq {
            return Err(bad(Reason::OutOfOrder));
        }
        if stored_prev != prev {
            return Err(bad(Reason::ChainBroken));
        }
        if format::state_digest(&state) != stored_digest {
            return Err(bad(Reason::StateDigestMismatch));
        }
        let recomputed = format::anchor_hash(&format::AnchorFields {
            seq,
            at_millis,
            head_hash: &head_hash,
            state_version,
            state_digest: &stored_digest,
            prev_anchor: &stored_prev,
        });
        if recomputed != stored_hash {
            return Err(bad(Reason::HashMismatch));
        }
        if !key.verify_anchor(&stored_hash, &signature) {
            return Err(bad(Reason::BadSignature));
        }
        prev_seq = seq;
        prev = stored_hash;
        anchors.push(Head {
            seq,
            hash: head_hash,
        });
    }
    Ok(anchors)
}

pub(crate) fn verify_detailed(
    conn: &Connection,
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<Detail, VerifyError> {
    let anchors = verify_anchors(conn, key)?;
    let mut stmt = conn.prepare(
        "SELECT seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature
         FROM events ORDER BY seq",
    )?;
    let mut rows = stmt.query([])?;

    // The chain starts at the beginning of history, or at the latest snapshot, which stands for
    // everything before it.
    let (starts_at, mut prev, mut head) = match anchors.last() {
        Some(last) => (last.seq + 1, last.hash, Some(*last)),
        None => (1, format::genesis_hash(key.as_bytes()), None),
    };
    let mut expected_seq = starts_at;
    while let Some(row) = rows.next()? {
        let seq = row
            .get::<_, i64>(0)
            .ok()
            .and_then(|s| u64::try_from(s).ok())
            .unwrap_or(0);
        let bad = |reason| VerifyError::Event { seq, reason };
        let malformed = |column| move |_| bad(Reason::Malformed(column));

        let at: i64 = row.get(1).map_err(malformed("at"))?;
        let origin_text: String = row.get(2).map_err(malformed("origin"))?;
        let kind: String = row.get(3).map_err(malformed("kind"))?;
        let schema_version: u32 = row.get(4).map_err(malformed("schema_version"))?;
        let payload: Vec<u8> = row.get(5).map_err(malformed("payload"))?;
        let stored_prev: Vec<u8> = row.get(6).map_err(malformed("prev_hash"))?;
        let stored_hash: Vec<u8> = row.get(7).map_err(malformed("hash"))?;
        let signature: Vec<u8> = row.get(8).map_err(malformed("signature"))?;

        let origin = Origin::parse(&origin_text).ok_or_else(|| bad(Reason::Malformed("origin")))?;
        let at_millis = u64::try_from(at).map_err(|_| bad(Reason::Malformed("at")))?;
        let stored_prev = Hash::try_from(stored_prev.as_slice())
            .map_err(|_| bad(Reason::Malformed("prev_hash")))?;
        let stored_hash =
            Hash::try_from(stored_hash.as_slice()).map_err(|_| bad(Reason::Malformed("hash")))?;

        if seq != expected_seq {
            return Err(bad(Reason::OutOfSequence {
                expected: expected_seq,
            }));
        }
        if stored_prev != prev {
            return Err(bad(Reason::ChainBroken));
        }
        let recomputed = format::event_hash(&Fields {
            seq,
            at_millis,
            origin,
            kind: &kind,
            schema_version,
            payload: &payload,
            prev_hash: &stored_prev,
        });
        if recomputed != stored_hash {
            return Err(bad(Reason::HashMismatch));
        }
        if !key.verify(&stored_hash, &signature) {
            return Err(bad(Reason::BadSignature));
        }
        prev = stored_hash;
        head = Some(Head {
            seq,
            hash: stored_hash,
        });
        expected_seq += 1;
    }

    if let Some(expected) = expect_head
        && head != Some(expected)
    {
        return Err(VerifyError::HeadMismatch {
            expected,
            found: head,
        });
    }
    Ok(Detail {
        verified: Verified {
            events: expected_seq - starts_at,
            head,
            anchored: anchors.last().copied(),
        },
        anchors,
        starts_at,
    })
}

/// A log checked together with its archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct History {
    pub live: Verified,
    /// How many archive files were checked.
    pub archives: usize,
    /// Whether every event from the very first was checked: either the log was never compacted, or the
    /// archives given cover everything the snapshots stand for. If not, the history before the latest
    /// snapshot rests on the engine's signature on it alone.
    pub from_genesis: bool,
}

/// [`verify`], and also each archive in `archives`. Every archive must verify on its own and must end
/// exactly where a snapshot of the live log stands, with that snapshot's hash, so an archive from
/// another log, or an altered one, is refused. Archives need not be given: the log verifies without
/// them, and [`History::from_genesis`] says whether the whole history was covered.
pub fn verify_history(
    live: &Path,
    archives: &[PathBuf],
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<History, VerifyError> {
    let open = |path: &Path| {
        Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
    };
    let detail = verify_detailed(&open(live)?, key, expect_head)?;

    let mut covered: Vec<(u64, u64)> = Vec::new();
    for path in archives {
        let wrap = |error| VerifyError::Archive {
            path: path.clone(),
            error: Box::new(error),
        };
        let archive =
            verify_detailed(&open(path).map_err(|e| wrap(e.into()))?, key, None).map_err(wrap)?;
        let end = archive
            .verified
            .head
            .filter(|_| archive.verified.events > 0);
        match end {
            Some(end) if detail.anchors.contains(&end) => {
                covered.push((archive.starts_at, end.seq))
            }
            other => {
                return Err(VerifyError::ArchiveNotAnchored {
                    path: path.clone(),
                    head: other,
                });
            }
        }
    }

    covered.sort_unstable();
    let mut next = 1u64;
    for (first, last) in covered {
        if first <= next {
            next = next.max(last + 1);
        }
    }
    let from_genesis = detail.anchors.last().is_none_or(|latest| next > latest.seq);
    Ok(History {
        live: detail.verified,
        archives: archives.len(),
        from_genesis,
    })
}
