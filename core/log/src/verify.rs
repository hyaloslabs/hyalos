//! Checking a log from the outside (ADR-0018): `hyalos log verify`.
//!
//! The verifier trusts nothing in the file. It is given the public key and, optionally, the head the
//! operator recorded earlier, and it opens the database read-only.

use std::fmt;
use std::path::Path;

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
        }
    }
}

#[derive(Debug)]
pub enum VerifyError {
    /// The first event that fails, by sequence number.
    Event { seq: u64, reason: Reason },
    /// Every event verifies, but the log ends somewhere other than where the operator recorded.
    HeadMismatch { expected: Head, found: Option<Head> },
    /// The file could not be opened or read as an event log.
    Storage(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Event { seq, reason } => write!(f, "event {seq}: {reason}"),
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
    pub events: u64,
    pub head: Option<Head>,
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
    let mut stmt = conn.prepare(
        "SELECT seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature
         FROM events ORDER BY seq",
    )?;
    let mut rows = stmt.query([])?;

    let mut expected_seq = 1u64;
    let mut prev = format::genesis_hash(key.as_bytes());
    let mut head: Option<Head> = None;
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
    Ok(Verified {
        events: expected_seq - 1,
        head,
    })
}
