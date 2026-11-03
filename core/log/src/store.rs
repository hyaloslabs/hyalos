//! Opening the log and appending to it (ADR-0003, ADR-0018).

use std::fmt;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use hyalos_types::{Origin, Timestamp};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::format::{self, Fields, Hash};
use crate::key::LogKey;

/// Bumped only with a new hash or table layout, together with an ADR.
pub const FORMAT_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS events (
    seq            INTEGER PRIMARY KEY,
    at             INTEGER NOT NULL,
    origin         TEXT    NOT NULL CHECK (origin IN ('engine', 'broker')),
    kind           TEXT    NOT NULL,
    schema_version INTEGER NOT NULL,
    payload        BLOB    NOT NULL,
    prev_hash      BLOB    NOT NULL CHECK (length(prev_hash) = 32),
    hash           BLOB    NOT NULL CHECK (length(hash) = 32),
    signature      BLOB    NOT NULL CHECK (length(signature) = 64)
) STRICT;

-- These stop a bug in the engine from rewriting history. They do not stop someone with the file:
-- that is what the hash chain and the signatures are for.
CREATE TRIGGER IF NOT EXISTS events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'the event log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'the event log is append-only'); END;
";

/// The last event of a log: how far it goes and the hash that commits to all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub seq: u64,
    pub hash: Hash,
}

impl fmt::Display for Head {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.seq, format::to_hex(&self.hash))
    }
}

/// One event as stored: everything but the hash and signature, which belong to verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: u64,
    pub at: Timestamp,
    pub origin: Origin,
    pub kind: String,
    pub schema_version: u32,
    pub payload: Vec<u8>,
}

impl Head {
    /// Parses the `seq:hash` form that [`Head`] prints, as an operator would record it.
    pub fn parse(text: &str) -> Option<Self> {
        let (seq, hash) = text.trim().split_once(':')?;
        Some(Self {
            seq: seq.parse().ok()?,
            hash: format::from_hex::<{ format::HASH_LEN }>(hash)?,
        })
    }
}

#[derive(Debug)]
pub enum LogError {
    /// SQLite or the filesystem failed.
    Storage(String),
    /// The log was made with a different signing key.
    WrongKey,
    /// The file uses a format this build does not know.
    UnsupportedFormat(i64),
    /// A value does not fit what the log can store.
    OutOfRange(&'static str),
    /// A stored value is not what the engine wrote. Run `hyalos log verify`.
    Corrupt(&'static str),
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::Storage(e) => write!(f, "event log storage error: {e}"),
            LogError::WrongKey => write!(f, "this event log was made with a different signing key"),
            LogError::UnsupportedFormat(v) => {
                write!(
                    f,
                    "event log format version {v} is not supported by this build"
                )
            }
            LogError::OutOfRange(what) => write!(f, "{what} is out of range for the event log"),
            LogError::Corrupt(what) => {
                write!(
                    f,
                    "the event log is damaged ({what}); run `hyalos log verify`"
                )
            }
        }
    }
}

impl std::error::Error for LogError {}

impl From<rusqlite::Error> for LogError {
    fn from(e: rusqlite::Error) -> Self {
        LogError::Storage(e.to_string())
    }
}

impl From<std::io::Error> for LogError {
    fn from(e: std::io::Error) -> Self {
        LogError::Storage(e.to_string())
    }
}

/// An open log. Only the engine process holds one.
#[derive(Debug)]
pub struct Log {
    conn: Connection,
    key: LogKey,
}

impl Log {
    /// Opens the log at `path`, creating it (readable by the owner only) if it does not exist.
    /// Fails with [`LogError::WrongKey`] if the log was made with a different key.
    pub fn open(path: &Path, key: LogKey) -> Result<Self, LogError> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "trusted_schema", "OFF")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('format_version', ?1)",
            [FORMAT_VERSION],
        )?;
        let version: i64 = conn.query_row(
            "SELECT value FROM meta WHERE key = 'format_version'",
            [],
            |r| r.get(0),
        )?;
        if version != FORMAT_VERSION {
            return Err(LogError::UnsupportedFormat(version));
        }
        let first_prev: Option<Vec<u8>> = conn
            .query_row("SELECT prev_hash FROM events WHERE seq = 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(prev) = first_prev
            && prev != format::genesis_hash(key.public().as_bytes())
        {
            return Err(LogError::WrongKey);
        }
        Ok(Self { conn, key })
    }

    /// Appends one event, hashing it onto the chain and signing it. `payload` is stored exactly as
    /// given. It must hold typed values, digests and references, never secrets or raw model text.
    pub fn append(
        &mut self,
        at: Timestamp,
        origin: Origin,
        kind: &str,
        schema_version: u32,
        payload: &[u8],
    ) -> Result<Head, LogError> {
        let at_millis = at.as_millis();
        let at_stored = i64::try_from(at_millis).map_err(|_| LogError::OutOfRange("timestamp"))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (seq, prev_hash) = match read_head(&tx)? {
            None => (1, format::genesis_hash(self.key.public().as_bytes())),
            Some(head) => (head.seq + 1, head.hash),
        };
        let hash = format::event_hash(&Fields {
            seq,
            at_millis,
            origin,
            kind,
            schema_version,
            payload,
            prev_hash: &prev_hash,
        });
        let signature = self.key.sign(&hash);
        tx.execute(
            "INSERT INTO events (seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                i64::try_from(seq).map_err(|_| LogError::OutOfRange("sequence number"))?,
                at_stored,
                origin.as_str(),
                kind,
                schema_version,
                payload,
                prev_hash.as_slice(),
                hash.as_slice(),
                signature.as_slice(),
            ],
        )?;
        tx.commit()?;
        Ok(Head { seq, hash })
    }

    /// The last event, or `None` for an empty log.
    pub fn head(&self) -> Result<Option<Head>, LogError> {
        read_head(&self.conn)
    }

    /// Every event in order, for rebuilding projections. It does not check hashes or signatures, so
    /// run [`crate::verify`] first if the file might have been touched.
    pub fn events(&self) -> Result<Vec<StoredEvent>, LogError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, origin, kind, schema_version, payload FROM events ORDER BY seq",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, Vec<u8>>(5)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, at, origin, kind, schema_version, payload) = row?;
            events.push(StoredEvent {
                seq: u64::try_from(seq).map_err(|_| LogError::Corrupt("sequence number"))?,
                at: Timestamp::from_millis(
                    u64::try_from(at).map_err(|_| LogError::Corrupt("timestamp"))?,
                ),
                origin: Origin::parse(&origin).ok_or(LogError::Corrupt("origin"))?,
                kind,
                schema_version,
                payload,
            });
        }
        Ok(events)
    }
}

fn read_head(conn: &Connection) -> Result<Option<Head>, LogError> {
    let row: Option<(i64, Vec<u8>)> = conn
        .query_row(
            "SELECT seq, hash FROM events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(seq, hash)| {
        Ok(Head {
            seq: u64::try_from(seq).map_err(|_| LogError::Corrupt("sequence number"))?,
            hash: Hash::try_from(hash.as_slice()).map_err(|_| LogError::Corrupt("hash length"))?,
        })
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_parses_from_what_it_prints() {
        let head = Head {
            seq: 42,
            hash: [0xAB; 32],
        };
        assert_eq!(Head::parse(&head.to_string()), Some(head));
        assert_eq!(Head::parse(&format!("{head}\n")), Some(head));
    }

    #[test]
    fn a_malformed_head_is_refused() {
        for bad in ["", "42", "42:", ":ab", "x:ab", "42:zz", "42:abab"] {
            assert_eq!(Head::parse(bad), None, "{bad:?}");
        }
    }
}
