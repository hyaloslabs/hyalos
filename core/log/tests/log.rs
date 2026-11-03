//! S1.1 acceptance: tampering with any stored event fails verification naming the event, and the
//! origin of every event is recorded (ADR-0003, ADR-0018).
//!
//! Tampering here is what an attacker with the file would do: open it with SQLite, drop the triggers
//! that guard against engine bugs, and edit rows. Only the signing key is out of their reach.

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hyalos_log::{
    Head, Log, LogError, LogKey, Origin, PublicKey, Reason, VerifyError, format, verify,
};
use hyalos_types::Timestamp;
use rusqlite::Connection;

type Res<T = ()> = Result<T, Box<dyn Error>>;

const EVENTS: u64 = 6;

fn key() -> LogKey {
    LogKey::from_seed([7; 32])
}

fn fresh_path(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("log-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

/// A log of `EVENTS` events. Every third event, starting with the third, comes from the broker.
fn filled(name: &str) -> Res<(PathBuf, Vec<Head>)> {
    let path = fresh_path(name)?;
    let mut log = Log::open(&path, key())?;
    let mut heads = Vec::new();
    for i in 1..=EVENTS {
        let origin = if i % 3 == 0 {
            Origin::Broker
        } else {
            Origin::Engine
        };
        let payload = format!("{{\"n\":{i}}}");
        heads.push(log.append(
            Timestamp::from_millis(1_000 * i),
            origin,
            &format!("test.event.{i}"),
            1,
            payload.as_bytes(),
        )?);
    }
    Ok((path, heads))
}

/// Opens the file the way an attacker would, with the append-only triggers removed.
fn attacker(path: &Path) -> Res<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("DROP TRIGGER events_no_update; DROP TRIGGER events_no_delete;")?;
    Ok(conn)
}

fn public() -> PublicKey {
    key().public()
}

fn failure(path: &Path) -> Res<(u64, Reason)> {
    match verify(path, &public(), None) {
        Err(VerifyError::Event { seq, reason }) => Ok((seq, reason)),
        other => Err(format!("expected an event failure, got {other:?}").into()),
    }
}

#[test]
fn an_untouched_log_verifies() -> Res {
    let (path, heads) = filled("untouched")?;
    let v = verify(&path, &public(), None)?;
    assert_eq!(v.events, EVENTS);
    assert_eq!(v.head, heads.last().copied());
    assert_eq!(heads[0].seq, 1);
    assert_eq!(heads[5].seq, 6);
    Ok(())
}

#[test]
fn editing_any_column_of_an_event_names_that_event() -> Res {
    let cases: [(&str, &str, Reason); 8] = [
        ("at", "at = at + 1", Reason::HashMismatch),
        (
            "origin",
            "origin = CASE origin WHEN 'engine' THEN 'broker' ELSE 'engine' END",
            Reason::HashMismatch,
        ),
        ("kind", "kind = kind || 'x'", Reason::HashMismatch),
        (
            "schema_version",
            "schema_version = schema_version + 1",
            Reason::HashMismatch,
        ),
        ("payload", "payload = x'00'", Reason::HashMismatch),
        ("prev_hash", "prev_hash = zeroblob(32)", Reason::ChainBroken),
        ("hash", "hash = zeroblob(32)", Reason::HashMismatch),
        (
            "signature",
            "signature = zeroblob(64)",
            Reason::BadSignature,
        ),
    ];
    for (name, set, expected) in cases {
        let (path, _) = filled(&format!("edit-{name}"))?;
        attacker(&path)?.execute(&format!("UPDATE events SET {set} WHERE seq = 3"), [])?;
        assert_eq!(failure(&path)?, (3, expected), "editing `{name}`");
    }
    Ok(())
}

#[test]
fn recomputing_every_hash_still_fails_because_the_attacker_cannot_sign() -> Res {
    let (path, _) = filled("rehash")?;
    let conn = attacker(&path)?;
    // Change event 3's payload, then rebuild the hash chain from there as a careful attacker would.
    conn.execute("UPDATE events SET payload = x'deadbeef' WHERE seq = 3", [])?;
    let mut prev: format::Hash = {
        let h: Vec<u8> =
            conn.query_row("SELECT hash FROM events WHERE seq = 2", [], |r| r.get(0))?;
        h.as_slice().try_into()?
    };
    for seq in 3..=EVENTS {
        let (at, kind, schema, payload): (i64, String, u32, Vec<u8>) = conn.query_row(
            "SELECT at, kind, schema_version, payload FROM events WHERE seq = ?1",
            [i64::try_from(seq)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let origin = if seq % 3 == 0 {
            Origin::Broker
        } else {
            Origin::Engine
        };
        let hash = format::event_hash(&format::Fields {
            seq,
            at_millis: u64::try_from(at)?,
            origin,
            kind: &kind,
            schema_version: schema,
            payload: &payload,
            prev_hash: &prev,
        });
        conn.execute(
            "UPDATE events SET prev_hash = ?1, hash = ?2 WHERE seq = ?3",
            rusqlite::params![prev.as_slice(), hash.as_slice(), i64::try_from(seq)?],
        )?;
        prev = hash;
    }
    assert_eq!(failure(&path)?, (3, Reason::BadSignature));
    Ok(())
}

#[test]
fn removing_an_event_from_the_middle_names_the_event_after_the_gap() -> Res {
    let (path, _) = filled("remove-middle")?;
    attacker(&path)?.execute("DELETE FROM events WHERE seq = 3", [])?;
    assert_eq!(failure(&path)?, (4, Reason::OutOfSequence { expected: 3 }));
    Ok(())
}

#[test]
fn removing_the_first_event_is_noticed() -> Res {
    let (path, _) = filled("remove-first")?;
    attacker(&path)?.execute("DELETE FROM events WHERE seq = 1", [])?;
    assert_eq!(failure(&path)?, (2, Reason::OutOfSequence { expected: 1 }));
    Ok(())
}

#[test]
fn swapping_two_events_is_noticed() -> Res {
    let (path, _) = filled("swap")?;
    attacker(&path)?.execute_batch(
        "UPDATE events SET seq = -1 WHERE seq = 3;
         UPDATE events SET seq = 3 WHERE seq = 4;
         UPDATE events SET seq = 4 WHERE seq = -1;",
    )?;
    assert_eq!(failure(&path)?, (3, Reason::ChainBroken));
    Ok(())
}

#[test]
fn a_log_verified_with_the_wrong_key_fails_at_the_first_event() -> Res {
    let (path, _) = filled("wrong-verify-key")?;
    let other = LogKey::from_seed([8; 32]).public();
    match verify(&path, &other, None) {
        Err(VerifyError::Event {
            seq: 1,
            reason: Reason::ChainBroken,
        }) => Ok(()),
        other => Err(format!("unexpected: {other:?}").into()),
    }
}

#[test]
fn removing_events_from_the_end_is_only_caught_by_an_expected_head() -> Res {
    let (path, heads) = filled("truncate")?;
    attacker(&path)?.execute("DELETE FROM events WHERE seq > 4", [])?;

    // The chain that remains is intact, so verification alone cannot tell.
    let v = verify(&path, &public(), None)?;
    assert_eq!(v.events, 4);

    // The head the operator recorded earlier exposes it.
    match verify(&path, &public(), heads.last().copied()) {
        Err(VerifyError::HeadMismatch { expected, found }) => {
            assert_eq!(expected.seq, EVENTS);
            assert_eq!(found.map(|h| h.seq), Some(4));
        }
        other => return Err(format!("unexpected: {other:?}").into()),
    }
    // And a head that is still the end passes.
    verify(&path, &public(), Some(heads[3]))?;
    Ok(())
}

#[test]
fn an_empty_log_verifies_but_not_against_a_recorded_head() -> Res {
    let path = fresh_path("empty")?;
    let log = Log::open(&path, key())?;
    assert_eq!(log.head()?, None);
    let v = verify(&path, &public(), None)?;
    assert_eq!((v.events, v.head), (0, None));
    let recorded = Head {
        seq: 1,
        hash: [1; 32],
    };
    assert!(matches!(
        verify(&path, &public(), Some(recorded)),
        Err(VerifyError::HeadMismatch { found: None, .. })
    ));
    Ok(())
}

#[test]
fn every_event_records_its_origin() -> Res {
    let (path, _) = filled("origin")?;
    let conn = Connection::open(&path)?;
    let mut stmt = conn.prepare("SELECT seq, origin FROM events ORDER BY seq")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    assert_eq!(rows.len(), EVENTS as usize);
    for (seq, origin) in rows {
        let expected = if seq % 3 == 0 { "broker" } else { "engine" };
        assert_eq!(origin, expected, "event {seq}");
    }
    Ok(())
}

#[test]
fn the_engines_own_bugs_cannot_rewrite_history() -> Res {
    let (path, _) = filled("append-only")?;
    let conn = Connection::open(&path)?;
    let update = conn.execute("UPDATE events SET kind = 'x' WHERE seq = 1", []);
    let delete = conn.execute("DELETE FROM events WHERE seq = 1", []);
    for result in [update, delete] {
        let message = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("append-only"), "got: {message:?}");
    }
    verify(&path, &public(), None)?;
    Ok(())
}

#[test]
fn a_log_can_be_verified_while_the_engine_still_has_it_open() -> Res {
    let path = fresh_path("live")?;
    let mut log = Log::open(&path, key())?;
    let mut last = None;
    for i in 1..=5u64 {
        last = Some(log.append(Timestamp::from_millis(i), Origin::Engine, "live", 1, b"{}")?);
    }
    // The log is still open, so recent events may be only in the -wal file.
    let v = verify(&path, &public(), last)?;
    assert_eq!(v.events, 5);
    // And the engine can keep appending afterwards.
    log.append(Timestamp::from_millis(6), Origin::Engine, "live", 1, b"{}")?;
    assert_eq!(verify(&path, &public(), None)?.events, 6);
    Ok(())
}

#[test]
fn reopening_continues_the_same_chain() -> Res {
    let path = fresh_path("reopen")?;
    {
        let mut log = Log::open(&path, key())?;
        log.append(Timestamp::from_millis(1), Origin::Engine, "a", 1, b"1")?;
        log.append(Timestamp::from_millis(2), Origin::Engine, "b", 1, b"2")?;
    }
    let mut log = Log::open(&path, key())?;
    let head = log.append(Timestamp::from_millis(3), Origin::Broker, "c", 1, b"3")?;
    assert_eq!(head.seq, 3);
    let v = verify(&path, &public(), Some(head))?;
    assert_eq!(v.events, 3);
    Ok(())
}

#[test]
fn a_log_cannot_be_opened_with_a_different_key() -> Res {
    let (path, _) = filled("wrong-open-key")?;
    let err = Log::open(&path, LogKey::from_seed([8; 32])).err();
    assert!(matches!(err, Some(LogError::WrongKey)), "got {err:?}");
    Ok(())
}

#[test]
fn the_file_is_created_readable_by_the_owner_only() -> Res {
    let path = fresh_path("mode")?;
    let mut log = Log::open(&path, key())?;
    log.append(Timestamp::from_millis(1), Origin::Engine, "m", 1, b"{}")?;
    // While the log is open SQLite also keeps -wal and -shm files, which hold recent events.
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.clone().into_os_string();
        name.push(suffix);
        let mode = std::fs::metadata(PathBuf::from(name))?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file with suffix {suffix:?}");
    }
    Ok(())
}

#[test]
fn payloads_are_stored_exactly_as_given() -> Res {
    let path = fresh_path("payloads")?;
    let mut log = Log::open(&path, key())?;
    let big = vec![0xA5u8; 1_000_000];
    for (i, payload) in [&b""[..], &[0u8, 255, 0, 1][..], &big[..]]
        .into_iter()
        .enumerate()
    {
        log.append(
            Timestamp::from_millis(i as u64),
            Origin::Engine,
            "p",
            1,
            payload,
        )?;
    }
    drop(log);
    verify(&path, &public(), None)?;
    let stored: Vec<u8> = Connection::open(&path)?.query_row(
        "SELECT payload FROM events WHERE seq = 2",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(stored, [0u8, 255, 0, 1]);
    Ok(())
}

#[test]
fn a_timestamp_the_log_cannot_store_is_refused() -> Res {
    let path = fresh_path("time-range")?;
    let mut log = Log::open(&path, key())?;
    let err = log
        .append(
            Timestamp::from_millis(u64::MAX),
            Origin::Engine,
            "t",
            1,
            b"",
        )
        .err();
    assert!(matches!(err, Some(LogError::OutOfRange(_))), "got {err:?}");
    assert_eq!(
        log.head()?,
        None,
        "a refused append must leave no event behind"
    );
    Ok(())
}

#[test]
fn a_missing_or_foreign_file_is_a_storage_error_not_a_pass() -> Res {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("log-foreign");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let missing = dir.join("nope.db");
    assert!(matches!(
        verify(&missing, &public(), None),
        Err(VerifyError::Storage(_))
    ));
    let junk = dir.join("junk.db");
    std::fs::write(&junk, b"this is not a database at all, just text")?;
    assert!(matches!(
        verify(&junk, &public(), None),
        Err(VerifyError::Storage(_))
    ));
    Ok(())
}
