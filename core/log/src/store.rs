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

/// The layout of a log that has never been compacted. A new log is written at this version, so a build
/// from before compaction can still read it.
pub const BASE_FORMAT_VERSION: i64 = 1;

/// The layout of a log that has been compacted: its events start after a snapshot (ADR-0026). The
/// first compaction sets it, so a build that does not know snapshots refuses the file instead of
/// misreading it. This is the highest version this build reads. Bumped only with a new hash or table
/// layout, together with an ADR.
pub const FORMAT_VERSION: i64 = 2;

/// Kept in one place so compaction can put it back exactly as `open` creates it.
pub(crate) const NO_DELETE_EVENTS: &str =
    "CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'the event log is append-only'); END;";

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

-- Snapshots (ADR-0026). One is written when events are compacted away: it commits to the last event
-- removed and to the state that followed from everything up to it. They are never updated or deleted,
-- and each commits to the one before, so none can be removed unseen.
CREATE TABLE IF NOT EXISTS anchors (
    seq           INTEGER PRIMARY KEY,
    at            INTEGER NOT NULL,
    head_hash     BLOB    NOT NULL CHECK (length(head_hash) = 32),
    state_version INTEGER NOT NULL,
    state         BLOB    NOT NULL,
    state_digest  BLOB    NOT NULL CHECK (length(state_digest) = 32),
    prev_anchor   BLOB    NOT NULL CHECK (length(prev_anchor) = 32),
    hash          BLOB    NOT NULL CHECK (length(hash) = 32),
    signature     BLOB    NOT NULL CHECK (length(signature) = 64)
) STRICT;
CREATE TRIGGER IF NOT EXISTS anchors_no_update BEFORE UPDATE ON anchors
BEGIN SELECT RAISE(ABORT, 'snapshots are never changed'); END;
CREATE TRIGGER IF NOT EXISTS anchors_no_delete BEFORE DELETE ON anchors
BEGIN SELECT RAISE(ABORT, 'snapshots are never removed'); END;
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

/// An event to append: everything the log stores except the hash chain fields.
#[derive(Debug, Clone, Copy)]
pub struct NewEvent<'a> {
    pub at: Timestamp,
    pub origin: Origin,
    pub kind: &'a str,
    pub schema_version: u32,
    pub payload: &'a [u8],
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
    /// `append_all` was given no events.
    EmptyBatch,
    /// Another process appended since the head the caller knew (ADR-0069). Nothing was written; the
    /// caller reads what it missed with [`Log::events_after`] and asks again.
    Moved,
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
            LogError::EmptyBatch => write!(f, "there are no events to append"),
            LogError::Moved => write!(f, "another process appended to the event log first"),
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
    pub(crate) conn: Connection,
    pub(crate) key: LogKey,
}

impl Log {
    /// The public half of the key this log signs with.
    pub fn public_key(&self) -> crate::PublicKey {
        self.key.public()
    }

    /// Verifies every event in this log, through the connection it was opened with. Unlike
    /// [`crate::verify`], which opens a path, this checks the very file this `Log` will read and write.
    pub fn verify(&self, key: &crate::PublicKey) -> Result<crate::Verified, crate::VerifyError> {
        crate::verify::verify_connection(&self.conn, key, None)
    }

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
        conn.execute_batch(NO_DELETE_EVENTS)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('format_version', ?1)",
            [BASE_FORMAT_VERSION],
        )?;
        let version: i64 = conn.query_row(
            "SELECT value FROM meta WHERE key = 'format_version'",
            [],
            |r| r.get(0),
        )?;
        if !(BASE_FORMAT_VERSION..=FORMAT_VERSION).contains(&version) {
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
        // A compacted log no longer holds event 1, but its first snapshot is bound to the key the same way.
        let first_anchor_prev: Option<Vec<u8>> = conn
            .query_row(
                "SELECT prev_anchor FROM anchors ORDER BY seq LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prev) = first_anchor_prev
            && prev != format::first_anchor_prev(key.public().as_bytes())
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
        self.append_all(&[NewEvent {
            at,
            origin,
            kind,
            schema_version,
            payload,
        }])
    }

    /// Appends several events in one transaction: all of them are written, in order, or none is. It
    /// returns the head after the last one. An empty batch is an error.
    pub fn append_all(&mut self, events: &[NewEvent<'_>]) -> Result<Head, LogError> {
        self.append_checked(None, events)
    }

    /// [`Log::append_all`], but only if the log still ends at `after` (`None`: it is empty). Several
    /// processes may write one log, each projecting it in memory, so a writer appends only onto the
    /// events it has seen; otherwise it gets [`LogError::Moved`] and nothing is written (ADR-0069).
    pub fn append_all_after(
        &mut self,
        after: Option<Head>,
        events: &[NewEvent<'_>],
    ) -> Result<Head, LogError> {
        self.append_checked(Some(after), events)
    }

    fn append_checked(
        &mut self,
        after: Option<Option<Head>>,
        events: &[NewEvent<'_>],
    ) -> Result<Head, LogError> {
        if events.is_empty() {
            return Err(LogError::EmptyBatch);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let head = read_head(&tx)?;
        if after.is_some_and(|after| after != head) {
            return Err(LogError::Moved);
        }
        let (mut seq, mut prev_hash) = match head {
            None => (0, format::genesis_hash(self.key.public().as_bytes())),
            Some(head) => (head.seq, head.hash),
        };
        for event in events {
            seq += 1;
            let at_millis = event.at.as_millis();
            let at_stored =
                i64::try_from(at_millis).map_err(|_| LogError::OutOfRange("timestamp"))?;
            let hash = format::event_hash(&Fields {
                seq,
                at_millis,
                origin: event.origin,
                kind: event.kind,
                schema_version: event.schema_version,
                payload: event.payload,
                prev_hash: &prev_hash,
            });
            let signature = self.key.sign(&hash);
            tx.execute(
                "INSERT INTO events (seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    i64::try_from(seq).map_err(|_| LogError::OutOfRange("sequence number"))?,
                    at_stored,
                    event.origin.as_str(),
                    event.kind,
                    event.schema_version,
                    event.payload,
                    prev_hash.as_slice(),
                    hash.as_slice(),
                    signature.as_slice(),
                ],
            )?;
            prev_hash = hash;
        }
        tx.commit()?;
        Ok(Head {
            seq,
            hash: prev_hash,
        })
    }

    /// The last event, or `None` for an empty log.
    pub fn head(&self) -> Result<Option<Head>, LogError> {
        read_head(&self.conn)
    }

    /// The head as it stood at event `seq`: that event, or the snapshot standing there. `None` for 0.
    pub fn head_at(&self, seq: u64) -> Result<Option<Head>, LogError> {
        if seq == 0 {
            return Ok(None);
        }
        let at = i64::try_from(seq).map_err(|_| LogError::OutOfRange("sequence number"))?;
        let hash: Option<Vec<u8>> = self
            .conn
            .query_row("SELECT hash FROM events WHERE seq = ?1", [at], |r| r.get(0))
            .optional()?;
        let head = match hash {
            Some(hash) => Some(Head {
                seq,
                hash: Hash::try_from(hash.as_slice())
                    .map_err(|_| LogError::Corrupt("hash length"))?,
            }),
            None => read_latest_anchor(&self.conn)?
                .map(|a| a.head())
                .filter(|h| h.seq == seq),
        };
        head.map(Some).ok_or(LogError::Corrupt(
            "no event or snapshot where the state ends",
        ))
    }

    /// The events appended after `after`, checked as `verify` checks them: in sequence from it,
    /// chained to its hash and signed with this log's key. With them comes the new head. It is how a
    /// writer catches up with another process's appends (ADR-0069); a gap, such as a compaction since
    /// `after`, is [`LogError::Corrupt`], and the caller opens the log afresh.
    pub fn events_after(
        &self,
        after: Option<Head>,
    ) -> Result<(Vec<StoredEvent>, Option<Head>), LogError> {
        let key = self.key.public();
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare(
            "SELECT seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature
             FROM events WHERE seq > ?1 ORDER BY seq",
        )?;
        let from = i64::try_from(after.map_or(0, |h| h.seq))
            .map_err(|_| LogError::OutOfRange("sequence number"))?;
        let mut rows = stmt.query([from])?;
        let mut head = after;
        let mut prev = after.map_or_else(|| format::genesis_hash(key.as_bytes()), |h| h.hash);
        let mut events = Vec::new();
        while let Some(row) = rows.next()? {
            let seq = u64::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| LogError::Corrupt("sequence number"))?;
            let at =
                u64::try_from(row.get::<_, i64>(1)?).map_err(|_| LogError::Corrupt("timestamp"))?;
            let origin =
                Origin::parse(&row.get::<_, String>(2)?).ok_or(LogError::Corrupt("origin"))?;
            let kind: String = row.get(3)?;
            let schema_version: u32 = row.get(4)?;
            let payload: Vec<u8> = row.get(5)?;
            let hash_of = |bytes: Vec<u8>| {
                Hash::try_from(bytes.as_slice()).map_err(|_| LogError::Corrupt("hash length"))
            };
            let stored_prev = hash_of(row.get(6)?)?;
            let stored_hash = hash_of(row.get(7)?)?;
            let signature: Vec<u8> = row.get(8)?;
            if seq != head.map_or(1, |h| h.seq + 1) {
                return Err(LogError::Corrupt(
                    "events missing since the head this process knew",
                ));
            }
            if stored_prev != prev {
                return Err(LogError::Corrupt("hash chain"));
            }
            let recomputed = format::event_hash(&Fields {
                seq,
                at_millis: at,
                origin,
                kind: &kind,
                schema_version,
                payload: &payload,
                prev_hash: &stored_prev,
            });
            if recomputed != stored_hash || !key.verify(&stored_hash, &signature) {
                return Err(LogError::Corrupt("event hash or signature"));
            }
            prev = stored_hash;
            head = Some(Head {
                seq,
                hash: stored_hash,
            });
            events.push(StoredEvent {
                seq,
                at: Timestamp::from_millis(at),
                origin,
                kind,
                schema_version,
                payload,
            });
        }
        Ok((events, head))
    }

    /// The events that follow the latest snapshot, in order (all of them if there is none), for
    /// rebuilding projections. It does not check hashes or signatures, so run [`crate::verify`] first
    /// if the file might have been touched.
    pub fn events(&self) -> Result<Vec<StoredEvent>, LogError> {
        read_events(&self.conn)
    }

    /// The latest snapshot, if events were ever compacted (ADR-0026). Like [`Log::events`] it is read
    /// as stored; verification is separate.
    pub fn latest_anchor(&self) -> Result<Option<Anchor>, LogError> {
        read_latest_anchor(&self.conn)
    }

    /// Whether a snapshot of this log stands at exactly `head`: the last event it covers, and that
    /// event's hash. An archive made from this log ends at one.
    pub fn has_anchor(&self, head: Head) -> Result<bool, LogError> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM anchors WHERE seq = ?1 AND head_hash = ?2",
                rusqlite::params![head.seq as i64, head.hash.as_slice()],
                |r| r.get(0),
            )
            .map_err(LogError::from)?;
        Ok(n > 0)
    }
}

/// A snapshot as stored: what the log guarantees about it, and the state bytes it carries. The log
/// never reads the state; the engine writes and reads it (ADR-0026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// The last event it covers. The events that remain start at `seq + 1`.
    pub seq: u64,
    /// When the compaction was done.
    pub at: Timestamp,
    /// The hash of event `seq`.
    pub head_hash: Hash,
    /// Which layout `state` is in.
    pub state_version: u32,
    pub state: Vec<u8>,
    /// The hash the engine signed.
    pub hash: Hash,
}

impl Anchor {
    /// The event this snapshot stands in for the end of.
    pub fn head(&self) -> Head {
        Head {
            seq: self.seq,
            hash: self.head_hash,
        }
    }
}

const ANCHOR_COLUMNS: &str = "seq, at, head_hash, state_version, state, hash";

fn anchor_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Anchor, LogError>> {
    let (seq, at, head_hash, state_version, state, hash): (
        i64,
        i64,
        Vec<u8>,
        u32,
        Vec<u8>,
        Vec<u8>,
    ) = (
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
    );
    Ok((|| {
        Ok(Anchor {
            seq: u64::try_from(seq).map_err(|_| LogError::Corrupt("snapshot sequence number"))?,
            at: Timestamp::from_millis(
                u64::try_from(at).map_err(|_| LogError::Corrupt("snapshot timestamp"))?,
            ),
            head_hash: Hash::try_from(head_hash.as_slice())
                .map_err(|_| LogError::Corrupt("snapshot hash length"))?,
            state_version,
            state,
            hash: Hash::try_from(hash.as_slice())
                .map_err(|_| LogError::Corrupt("snapshot hash length"))?,
        })
    })())
}

pub(crate) fn read_latest_anchor(conn: &Connection) -> Result<Option<Anchor>, LogError> {
    conn.query_row(
        &format!("SELECT {ANCHOR_COLUMNS} FROM anchors ORDER BY seq DESC LIMIT 1"),
        [],
        anchor_from_row,
    )
    .optional()?
    .transpose()
}

/// Every snapshot, oldest first.
pub(crate) fn read_anchors(conn: &Connection) -> Result<Vec<Anchor>, LogError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ANCHOR_COLUMNS} FROM anchors ORDER BY seq"
    ))?;
    let rows = stmt.query_map([], anchor_from_row)?;
    rows.map(|row| row?).collect()
}

pub(crate) fn read_events(conn: &Connection) -> Result<Vec<StoredEvent>, LogError> {
    let mut stmt = conn.prepare(
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

/// The end of the chain: the last event, or, when compaction has removed them all, the last snapshot.
pub(crate) fn read_head(conn: &Connection) -> Result<Option<Head>, LogError> {
    let row: Option<(i64, Vec<u8>)> = conn
        .query_row(
            "SELECT seq, hash FROM events ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((seq, hash)) => Ok(Some(Head {
            seq: u64::try_from(seq).map_err(|_| LogError::Corrupt("sequence number"))?,
            hash: Hash::try_from(hash.as_slice()).map_err(|_| LogError::Corrupt("hash length"))?,
        })),
        None => Ok(read_latest_anchor(conn)?.map(|a| a.head())),
    }
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
