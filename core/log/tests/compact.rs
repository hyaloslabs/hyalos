//! S1.7: compaction replaces events with a signed snapshot, keeps the chain, archives what it removes,
//! and tampering with any of it is caught (ADR-0003, ADR-0026).

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hyalos_log::format::{self, AnchorFields};
use hyalos_log::{
    CompactError, CopyError, Head, Log, LogError, LogKey, Origin, Reason, StoredEvent, VerifyError,
    copy_verified, verify, verify_history,
};
use hyalos_types::Timestamp;
use rusqlite::Connection;

type Res<T = ()> = Result<T, Box<dyn Error>>;

fn key() -> LogKey {
    LogKey::from_seed([7; 32])
}

fn other_key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("compact-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn add(log: &mut Log, n: u64) -> Res<Head> {
    let origin = if n.is_multiple_of(3) {
        Origin::Broker
    } else {
        Origin::Engine
    };
    Ok(log.append(
        Timestamp::from_millis(1_000 * n),
        origin,
        &format!("test.event.{n}"),
        1,
        format!("{{\"n\":{n}}}").as_bytes(),
    )?)
}

/// A log holding events `1..=n`, and the head after each.
fn filled(path: &Path, n: u64) -> Res<(Log, Vec<Head>)> {
    let mut log = Log::open(path, key())?;
    let mut heads = Vec::new();
    for i in 1..=n {
        heads.push(add(&mut log, i)?);
    }
    Ok((log, heads))
}

const STATE: &[u8] = b"{\"the\":\"state\"}";

/// Archives to `archive` and compacts, keeping `STATE`.
fn compact(log: &mut Log, archive: &Path, at: u64) -> Res<hyalos_log::Anchor> {
    let archived = log.archive(archive)?;
    Ok(
        log.compact::<String>(archived, Timestamp::from_millis(at), |_, _| {
            Ok((1, STATE.to_vec()))
        })?,
    )
}

/// An archive is read-only; someone who wants to change one starts by making it writable.
fn writable(path: &Path) -> Res {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn attacker(path: &Path) -> Res<Connection> {
    writable(path)?;
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "DROP TRIGGER events_no_update; DROP TRIGGER events_no_delete;
         DROP TRIGGER anchors_no_update; DROP TRIGGER anchors_no_delete;",
    )?;
    Ok(conn)
}

fn first_failure(path: &Path) -> Res<VerifyError> {
    verify(path, &key().public(), None)
        .err()
        .ok_or_else(|| "expected verification to fail".into())
}

// ---- the chain survives -------------------------------------------------------------------------

#[test]
fn compaction_keeps_the_head_and_the_chain_goes_on_from_it() -> Res {
    let d = dir("chain")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 6)?;
    let last = *heads.last().ok_or("no heads")?;

    let anchor = compact(&mut log, &d.join("a1.db"), 9_000)?;
    assert_eq!((anchor.seq, anchor.head_hash), (6, last.hash));

    assert!(log.events()?.is_empty(), "the events were replaced");
    assert_eq!(log.head()?, Some(last), "the head is the snapshot's");
    let v = verify(&path, &key().public(), Some(last))?;
    assert_eq!((v.events, v.head, v.anchored), (0, Some(last), Some(last)));

    // The next event is event 7 and links to the hash of event 6, as if nothing had happened.
    let seventh = add(&mut log, 7)?;
    assert_eq!(seventh.seq, 7);
    drop(log);
    let prev: Vec<u8> = Connection::open(&path)?.query_row(
        "SELECT prev_hash FROM events WHERE seq = 7",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(prev, last.hash.to_vec());
    let v = verify(&path, &key().public(), Some(seventh))?;
    assert_eq!((v.events, v.anchored), (1, Some(last)));

    // Reopening carries on from the snapshot too.
    let mut log = Log::open(&path, key())?;
    assert_eq!(add(&mut log, 8)?.seq, 8);
    Ok(())
}

#[test]
fn a_compacted_log_and_one_that_never_was_have_the_same_head_at_every_step() -> Res {
    // Compaction must not change a single hash: the twin gets the same events and never compacts.
    let d = dir("twin")?;
    let mut compacted = Log::open(&d.join("a.db"), key())?;
    let mut twin = Log::open(&d.join("b.db"), key())?;
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let (mut n, mut compactions) = (0u64, 0);
    for step in 0..120 {
        if next() % 7 == 0 && !compacted.events()?.is_empty() {
            compact(
                &mut compacted,
                &d.join(format!("archive-{step}.db")),
                1_000_000 + step,
            )?;
            compactions += 1;
        } else {
            n += 1;
            assert_eq!(add(&mut compacted, n)?, add(&mut twin, n)?, "step {step}");
        }
        assert_eq!(compacted.head()?, twin.head()?, "step {step}");
        verify(&d.join("a.db"), &key().public(), compacted.head()?)?;
    }
    assert!(compactions >= 5, "{compactions} compactions");
    Ok(())
}

// ---- nothing is deleted -------------------------------------------------------------------------

#[test]
fn the_archive_holds_every_event_and_verifies_on_its_own() -> Res {
    let d = dir("archive")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 6)?;
    let before = log.events()?;
    let archive = d.join("a1.db");
    compact(&mut log, &archive, 9_000)?;
    assert_eq!(
        std::fs::metadata(&archive)?.permissions().mode() & 0o777,
        0o400,
        "an archive is made read-only"
    );

    let v = verify(&archive, &key().public(), heads.last().copied())?;
    assert_eq!((v.events, v.anchored), (6, None), "all six, from genesis");
    writable(&archive)?; // Log::open wants to write; reading the events does not
    assert_eq!(Log::open(&archive, key())?.events()?, before);
    Ok(())
}

#[test]
fn a_second_compaction_archives_only_what_came_after_the_first() -> Res {
    let d = dir("second")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 6)?;
    compact(&mut log, &d.join("a1.db"), 9_000)?;
    for n in 7..=9 {
        add(&mut log, n)?;
    }

    // The snapshot function sees the previous snapshot and only the events since.
    let archived = log.archive(&d.join("a2.db"))?;
    let seen = std::cell::RefCell::new(None);
    log.compact::<String>(archived, Timestamp::from_millis(10_000), |prior, events| {
        *seen.borrow_mut() = Some((
            prior.map(|p| (p.seq, p.state.clone())),
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        ));
        Ok((1, b"second".to_vec()))
    })?;
    assert_eq!(
        seen.into_inner(),
        Some((Some((6, STATE.to_vec())), vec![7, 8, 9]))
    );

    let second = verify(&d.join("a2.db"), &key().public(), None)?;
    assert_eq!(
        (second.events, second.anchored.map(|h| h.seq)),
        (3, Some(6))
    );
    assert_eq!(heads.len(), 6);
    let live = verify(&path, &key().public(), None)?;
    assert_eq!(live.anchored.map(|h| h.seq), Some(9));
    Ok(())
}

#[test]
fn archives_are_checked_against_the_snapshots_they_stand_behind() -> Res {
    let d = dir("history")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    let (a1, a2) = (d.join("a1.db"), d.join("a2.db"));
    compact(&mut log, &a1, 9_000)?;
    for n in 7..=9 {
        add(&mut log, n)?;
    }
    compact(&mut log, &a2, 10_000)?;
    add(&mut log, 10)?;
    let public = key().public();

    let both = verify_history(&path, &[a1.clone(), a2.clone()], &public, None)?;
    assert_eq!((both.archives, both.from_genesis), (2, true));
    let order = verify_history(&path, &[a2.clone(), a1.clone()], &public, None)?;
    assert!(order.from_genesis, "the order given does not matter");

    for partial in [vec![a2.clone()], vec![a1.clone()], vec![]] {
        let h = verify_history(&path, &partial, &public, None)?;
        assert!(!h.from_genesis, "{} archives", partial.len());
    }

    // An archive of some other log, even one that verifies, is not this log's history.
    let other = d.join("other.db");
    let (elsewhere, _) = filled(&other, 4)?;
    let foreign = d.join("foreign.db");
    elsewhere.archive(&foreign)?;
    let err = verify_history(&path, &[foreign], &public, None).err();
    assert!(
        matches!(err, Some(VerifyError::ArchiveNotAnchored { .. })),
        "{err:?}"
    );

    // The live log itself is not an archive of itself.
    let err = verify_history(&path, std::slice::from_ref(&path), &public, None).err();
    assert!(
        matches!(err, Some(VerifyError::ArchiveNotAnchored { .. })),
        "{err:?}"
    );
    Ok(())
}

#[test]
fn an_altered_archive_is_refused() -> Res {
    let d = dir("bad-archive")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    let a1 = d.join("a1.db");
    compact(&mut log, &a1, 9_000)?;
    attacker(&a1)?.execute("UPDATE events SET payload = x'7b7d' WHERE seq = 3", [])?;
    let err = verify_history(&path, &[a1], &key().public(), None).err();
    assert!(
        matches!(&err, Some(VerifyError::Archive { error, .. })
            if matches!(**error, VerifyError::Event { seq: 3, reason: Reason::HashMismatch })),
        "{err:?}"
    );
    Ok(())
}

// ---- tampering is caught ------------------------------------------------------------------------

#[test]
fn every_edit_to_a_snapshot_is_caught_without_the_key() -> Res {
    let edits: Vec<(&str, &str, Reason)> = vec![
        (
            "the state",
            "UPDATE anchors SET state = x'00' WHERE seq = 6",
            Reason::StateDigestMismatch,
        ),
        (
            "the head it stands for",
            "UPDATE anchors SET head_hash = zeroblob(32) WHERE seq = 6",
            Reason::HashMismatch,
        ),
        (
            "when it was made",
            "UPDATE anchors SET at = at + 1 WHERE seq = 6",
            Reason::HashMismatch,
        ),
        (
            "the state version",
            "UPDATE anchors SET state_version = 9 WHERE seq = 6",
            Reason::HashMismatch,
        ),
        (
            "the signature",
            "UPDATE anchors SET signature = zeroblob(64) WHERE seq = 6",
            Reason::BadSignature,
        ),
    ];
    for (what, sql, expected) in edits {
        let d = dir("edit-anchor")?;
        let path = d.join("events.db");
        let (mut log, _) = filled(&path, 6)?;
        compact(&mut log, &d.join("a.db"), 9_000)?;
        drop(log);
        attacker(&path)?.execute(sql, [])?;
        match first_failure(&path)? {
            VerifyError::Anchor { seq: 6, reason } => assert_eq!(reason, expected, "{what}"),
            other => return Err(format!("{what}: {other}").into()),
        }
    }
    Ok(())
}

#[test]
fn a_forged_snapshot_with_a_consistent_hash_still_has_no_valid_signature() -> Res {
    // The attacker swaps the state and recomputes the digest and the hash, which anyone can do.
    let d = dir("forge")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 6)?;
    let anchor = compact(&mut log, &d.join("a.db"), 9_000)?;
    drop(log);
    let forged = b"{\"the\":\"attacker's state\"}".to_vec();
    let digest = format::state_digest(&forged);
    let prev: Vec<u8> = Connection::open(&path)?.query_row(
        "SELECT prev_anchor FROM anchors WHERE seq = 6",
        [],
        |r| r.get(0),
    )?;
    let prev: [u8; 32] = prev.as_slice().try_into()?;
    let hash = format::anchor_hash(&AnchorFields {
        seq: 6,
        at_millis: anchor.at.as_millis(),
        head_hash: &heads[5].hash,
        state_version: 1,
        state_digest: &digest,
        prev_anchor: &prev,
    });
    attacker(&path)?.execute(
        "UPDATE anchors SET state = ?1, state_digest = ?2, hash = ?3 WHERE seq = 6",
        rusqlite::params![forged, digest.as_slice(), hash.as_slice()],
    )?;
    assert!(matches!(
        first_failure(&path)?,
        VerifyError::Anchor {
            seq: 6,
            reason: Reason::BadSignature
        }
    ));
    Ok(())
}

#[test]
fn removing_the_snapshot_while_events_follow_it_is_caught() -> Res {
    let d = dir("remove-anchor")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    add(&mut log, 7)?;
    drop(log);
    attacker(&path)?.execute("DELETE FROM anchors", [])?;
    assert!(matches!(
        first_failure(&path)?,
        VerifyError::Event {
            seq: 7,
            reason: Reason::OutOfSequence { expected: 1 }
        }
    ));
    Ok(())
}

#[test]
fn removing_an_earlier_snapshot_breaks_the_chain_of_snapshots() -> Res {
    let d = dir("remove-first")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a1.db"), 9_000)?;
    add(&mut log, 7)?;
    compact(&mut log, &d.join("a2.db"), 10_000)?;
    drop(log);
    attacker(&path)?.execute("DELETE FROM anchors WHERE seq = 6", [])?;
    assert!(matches!(
        first_failure(&path)?,
        VerifyError::Anchor {
            seq: 7,
            reason: Reason::ChainBroken
        }
    ));
    Ok(())
}

#[test]
fn an_event_after_the_snapshot_is_still_protected_by_its_own_hash() -> Res {
    let d = dir("event-after")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    for n in 7..=9 {
        add(&mut log, n)?;
    }
    drop(log);
    attacker(&path)?.execute("UPDATE events SET payload = x'7b7d' WHERE seq = 8", [])?;
    assert!(matches!(
        first_failure(&path)?,
        VerifyError::Event {
            seq: 8,
            reason: Reason::HashMismatch
        }
    ));
    Ok(())
}

#[test]
fn the_first_event_after_a_snapshot_must_link_to_it() -> Res {
    let d = dir("link")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    add(&mut log, 7)?;
    drop(log);
    attacker(&path)?.execute(
        "UPDATE events SET prev_hash = zeroblob(32) WHERE seq = 7",
        [],
    )?;
    assert!(matches!(
        first_failure(&path)?,
        VerifyError::Event {
            seq: 7,
            reason: Reason::ChainBroken
        }
    ));
    Ok(())
}

#[test]
fn snapshots_belong_to_one_key() -> Res {
    let d = dir("key")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    drop(log);
    // Verified with another public key, the very first snapshot already fails.
    let err = verify(&path, &other_key().public(), None).err();
    assert!(
        matches!(
            err,
            Some(VerifyError::Anchor {
                seq: 6,
                reason: Reason::ChainBroken
            })
        ),
        "{err:?}"
    );
    // And a log with no events left cannot be opened for writing by another key either.
    assert!(matches!(
        Log::open(&path, other_key()),
        Err(LogError::WrongKey)
    ));
    Ok(())
}

#[test]
fn a_snapshot_numbered_zero_is_refused_before_anything_else() -> Res {
    // Rows come back in sequence order, so the only way a snapshot can be out of order is a number
    // that cannot be real. The engine's signature could not cover it, but say so before checking.
    let d = dir("zero")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 3)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    drop(log);
    attacker(&path)?.execute(
        "INSERT INTO anchors (seq, at, head_hash, state_version, state, state_digest, prev_anchor, hash, signature)
         VALUES (0, 0, zeroblob(32), 1, x'', zeroblob(32), zeroblob(32), zeroblob(32), zeroblob(64))",
        [],
    )?;
    assert!(matches!(
        first_failure(&path)?,
        VerifyError::Anchor {
            seq: 0,
            reason: Reason::OutOfOrder
        }
    ));
    Ok(())
}

// ---- when it must not happen --------------------------------------------------------------------

#[test]
fn a_log_that_moved_on_after_it_was_archived_is_not_compacted() -> Res {
    let d = dir("moved")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 6)?;
    let archived = log.archive(&d.join("a1.db"))?;

    // Another writer adds an event the archive does not hold.
    let newer = add(&mut Log::open(&path, key())?, 7)?;
    let result = log.compact::<String>(archived, Timestamp::from_millis(9_000), |_, _| {
        Ok((1, STATE.to_vec()))
    });
    match result {
        Err(CompactError::Moved { archived: a, now }) => {
            assert_eq!((a, now), (heads[5], Some(newer)));
        }
        other => return Err(format!("expected Moved, got {other:?}").into()),
    }
    assert_eq!(log.events()?.len(), 7, "nothing was removed");
    assert!(log.latest_anchor()?.is_none());

    // Archiving again, under a new name, and compacting works.
    compact(&mut log, &d.join("a2.db"), 9_000)?;
    assert_eq!(log.head()?, Some(newer));
    Ok(())
}

#[test]
fn there_is_nothing_to_compact_in_an_empty_log_or_right_after_a_compaction() -> Res {
    let d = dir("nothing")?;
    let path = d.join("events.db");
    let mut log = Log::open(&path, key())?;
    let first = d.join("a0.db");
    assert!(matches!(
        log.archive(&first),
        Err(CompactError::NothingToCompact)
    ));
    assert!(!first.exists(), "no archive was made");

    for n in 1..=3 {
        add(&mut log, n)?;
    }
    compact(&mut log, &d.join("a1.db"), 9_000)?;
    let second = d.join("a2.db");
    assert!(matches!(
        log.archive(&second),
        Err(CompactError::NothingToCompact)
    ));
    assert!(!second.exists());
    Ok(())
}

#[test]
fn an_archive_is_never_overwritten() -> Res {
    let d = dir("exists")?;
    let path = d.join("events.db");
    let (log, _) = filled(&path, 3)?;
    let archive = d.join("a1.db");
    std::fs::write(&archive, b"precious")?;
    assert!(matches!(
        log.archive(&archive),
        Err(CompactError::Archive(CopyError::Exists(_)))
    ));
    assert_eq!(std::fs::read(&archive)?, b"precious");
    assert_eq!(log.events()?.len(), 3, "the live log is untouched");
    Ok(())
}

#[test]
fn a_failed_snapshot_changes_nothing_and_the_guard_against_deletes_stays() -> Res {
    let d = dir("fail")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 6)?;
    let archived = log.archive(&d.join("a1.db"))?;
    let result = log.compact(archived, Timestamp::from_millis(9_000), |_, _| {
        Err::<(u32, Vec<u8>), _>("cannot snapshot")
    });
    assert!(matches!(
        result,
        Err(CompactError::Snapshot("cannot snapshot"))
    ));
    assert_eq!(log.events()?.len(), 6);
    assert!(log.latest_anchor()?.is_none());
    assert_eq!(log.head()?, heads.last().copied());
    drop(log);
    let conn = Connection::open(&path)?;
    assert!(
        conn.execute("DELETE FROM events", []).is_err(),
        "the guard is still there"
    );
    Ok(())
}

#[test]
fn after_a_compaction_the_logs_own_guards_still_refuse_edits_and_deletes() -> Res {
    let d = dir("guards")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    add(&mut log, 7)?;
    drop(log);
    let conn = Connection::open(&path)?;
    for sql in [
        "DELETE FROM events",
        "UPDATE events SET kind = 'x'",
        "DELETE FROM anchors",
        "UPDATE anchors SET state = x'00'",
    ] {
        assert!(conn.execute(sql, []).is_err(), "{sql}");
    }
    Ok(())
}

// ---- format and what a snapshot carries -----------------------------------------------------------

#[test]
fn only_a_compaction_changes_the_format_version() -> Res {
    let d = dir("format")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 3)?;
    let version = |p: &Path| -> Res<i64> {
        Ok(Connection::open(p)?.query_row(
            "SELECT value FROM meta WHERE key = 'format_version'",
            [],
            |r| r.get(0),
        )?)
    };
    assert_eq!(
        version(&path)?,
        1,
        "so a build from before compaction still reads it"
    );
    compact(&mut log, &d.join("a.db"), 9_000)?;
    assert_eq!(
        version(&path)?,
        2,
        "so such a build refuses a compacted log"
    );
    drop(log);

    Connection::open(&path)?
        .execute("UPDATE meta SET value = 3 WHERE key = 'format_version'", [])?;
    assert!(matches!(
        Log::open(&path, key()),
        Err(LogError::UnsupportedFormat(3))
    ));
    Ok(())
}

#[test]
fn the_snapshot_is_stored_exactly_as_given() -> Res {
    let d = dir("stored")?;
    let path = d.join("events.db");
    let (mut log, heads) = filled(&path, 4)?;
    let given = compact(&mut log, &d.join("a.db"), 9_000)?;
    let read = log.latest_anchor()?.ok_or("no snapshot")?;
    assert_eq!(read, given);
    assert_eq!(
        (
            read.seq,
            read.at,
            read.head_hash,
            read.state_version,
            read.state.as_slice()
        ),
        (4, Timestamp::from_millis(9_000), heads[3].hash, 1, STATE)
    );
    Ok(())
}

#[test]
fn the_snapshot_function_sees_the_events_as_stored() -> Res {
    let d = dir("seen")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 4)?;
    let expected: Vec<StoredEvent> = log.events()?;
    let archived = log.archive(&d.join("a.db"))?;
    let seen = std::cell::RefCell::new(Vec::new());
    log.compact::<String>(archived, Timestamp::from_millis(9_000), |prior, events| {
        assert!(prior.is_none());
        *seen.borrow_mut() = events.to_vec();
        Ok((1, vec![]))
    })?;
    assert_eq!(seen.into_inner(), expected);
    Ok(())
}

// ---- backup ----------------------------------------------------------------------------------------

#[test]
fn a_compacted_log_backs_up_and_restores_and_carries_on() -> Res {
    let d = dir("backup")?;
    let path = d.join("events.db");
    let (mut log, _) = filled(&path, 6)?;
    compact(&mut log, &d.join("a.db"), 9_000)?;
    let head = add(&mut log, 7)?;
    drop(log);

    let backup = d.join("backup.db");
    let v = copy_verified(&path, &backup, &key().public(), Some(head))?;
    assert_eq!((v.events, v.anchored.map(|h| h.seq)), (1, Some(6)));

    let mut restored = Log::open(&backup, key())?;
    assert_eq!(restored.head()?, Some(head));
    assert_eq!(add(&mut restored, 8)?.seq, 8);
    Ok(())
}
