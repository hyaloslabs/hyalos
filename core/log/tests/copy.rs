//! S1.3: a copy of the log is consistent, verified before it exists, and never overwrites (ADR-0021).

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hyalos_log::{
    CopyError, Head, Log, LogKey, Origin, Reason, VerifyError, copy_verified, verify,
};
use hyalos_types::Timestamp;
use rusqlite::Connection;

type Res<T = ()> = Result<T, Box<dyn Error>>;

fn key() -> LogKey {
    LogKey::from_seed([5; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("copy-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn append(log: &mut Log, from: u64, to: u64) -> Res<Option<Head>> {
    let mut head = None;
    for i in from..=to {
        let origin = if i % 3 == 0 {
            Origin::Broker
        } else {
            Origin::Engine
        };
        head = Some(log.append(
            Timestamp::from_millis(1_000 * i),
            origin,
            &format!("test.event.{i}"),
            1,
            format!("{{\"n\":{i}}}").as_bytes(),
        )?);
    }
    Ok(head)
}

/// One stored event: seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature.
type Row = (
    i64,
    i64,
    String,
    String,
    i64,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
);

/// Every stored column of every event, in order, as raw values.
fn rows(path: &Path) -> Res<Vec<Row>> {
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare(
        "SELECT seq, at, origin, kind, schema_version, payload, prev_hash, hash, signature
         FROM events ORDER BY seq",
    )?;
    let out = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

/// Files a copy to `name` could leave behind: its temporary file and any journal beside it.
fn leftovers(dir: &Path, name: &str) -> Vec<String> {
    ["", ".partial", "-wal", "-shm", "-journal"]
        .iter()
        .map(|suffix| format!("{name}{suffix}"))
        .filter(|n| std::fs::symlink_metadata(dir.join(n)).is_ok())
        .collect()
}

fn source(name: &str, events: u64) -> Res<(PathBuf, PathBuf, Head)> {
    let d = dir(name)?;
    let src = d.join("events.db");
    let mut log = Log::open(&src, key())?;
    let head = append(&mut log, 1, events)?.ok_or("no events")?;
    Ok((d, src, head))
}

#[test]
fn a_copy_has_every_event_identical_and_verifies() -> Res {
    let (d, src, head) = source("identical", 8)?;
    let dst = d.join("backup.db");
    let verified = copy_verified(&src, &dst, &key().public(), Some(head))?;
    assert_eq!((verified.events, verified.head), (8, Some(head)));
    assert_eq!(
        rows(&dst)?,
        rows(&src)?,
        "every column of every event, including hashes and signatures"
    );
    assert_eq!(rows(&dst)?.len(), 8);
    Ok(())
}

#[test]
fn a_copy_is_one_owner_only_file_with_nothing_left_beside_it() -> Res {
    let (d, src, _) = source("shape", 4)?;
    let dst = d.join("backup.db");
    copy_verified(&src, &dst, &key().public(), None)?;
    assert_eq!(std::fs::metadata(&dst)?.permissions().mode() & 0o777, 0o600);
    // Only the copy itself exists: no temporary file and no -wal, -shm or journal beside it.
    assert_eq!(leftovers(&d, "backup.db"), ["backup.db"]);
    Ok(())
}

#[test]
fn a_copy_can_be_opened_and_carries_on_the_same_chain() -> Res {
    let (d, src, head) = source("carry-on", 5)?;
    let dst = d.join("backup.db");
    copy_verified(&src, &dst, &key().public(), None)?;
    let mut restored = Log::open(&dst, key())?;
    assert_eq!(restored.head()?, Some(head));
    let next = append(&mut restored, 6, 6)?.ok_or("no event")?;
    assert_eq!(next.seq, 6);
    verify(&dst, &key().public(), Some(next))?;
    // The source did not change.
    assert_eq!(Log::open(&src, key())?.head()?, Some(head));
    Ok(())
}

#[test]
fn events_still_in_the_write_ahead_log_are_copied() -> Res {
    // The reason this is not a file copy: with the engine's connection open, recent events may sit in
    // the -wal file, and copying events.db alone would leave them behind.
    let d = dir("wal")?;
    let src = d.join("events.db");
    let mut live = Log::open(&src, key())?;
    let head = append(&mut live, 1, 40)?.ok_or("no events")?;
    let wal = d.join("events.db-wal");
    assert!(
        std::fs::metadata(&wal)?.len() > 0,
        "the test needs events in the WAL"
    );
    let naive = d.join("naive.db");
    std::fs::copy(&src, &naive)?;
    let naive_events = Connection::open(&naive)?
        .query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0);

    let dst = d.join("backup.db");
    let verified = copy_verified(&src, &dst, &key().public(), Some(head))?;
    assert_eq!(verified.events, 40);
    assert!(
        naive_events < 40,
        "a plain file copy lost events ({naive_events} of 40), which is the point"
    );
    drop(live);
    Ok(())
}

#[test]
fn copies_taken_while_the_engine_writes_are_valid_prefixes() -> Res {
    let d = dir("concurrent")?;
    let src = d.join("events.db");
    let mut log = Log::open(&src, key())?;
    append(&mut log, 1, 5)?;
    let writer_src = src.clone();
    let writer = std::thread::spawn(move || -> Result<(), String> {
        let mut log = Log::open(&writer_src, key()).map_err(|e| e.to_string())?;
        append(&mut log, 6, 150).map_err(|e| e.to_string())?;
        Ok(())
    });
    let mut sizes = Vec::new();
    for i in 0..5 {
        let dst = d.join(format!("backup-{i}.db"));
        match copy_verified(&src, &dst, &key().public(), None) {
            Ok(v) => sizes.push(v.events),
            Err(e) => return Err(format!("copy {i} failed: {e}").into()),
        }
    }
    writer.join().map_err(|_| "writer panicked")??;
    assert!(
        sizes.windows(2).all(|w| w[0] <= w[1]),
        "each copy is the log as it was: {sizes:?}"
    );
    assert!(sizes.iter().all(|n| (5..=150).contains(n)), "{sizes:?}");
    // A copy is the chain up to some event, so it matches the source row for row up to there.
    let all = rows(&src)?;
    for i in 0..5 {
        let copy = rows(&d.join(format!("backup-{i}.db")))?;
        assert_eq!(copy[..], all[..copy.len()]);
    }
    Ok(())
}

#[test]
fn nothing_is_ever_overwritten() -> Res {
    let (d, src, _) = source("no-overwrite", 3)?;
    let dst = d.join("backup.db");
    std::fs::write(&dst, b"precious")?;
    assert!(matches!(
        copy_verified(&src, &dst, &key().public(), None),
        Err(CopyError::Exists(_))
    ));
    assert_eq!(std::fs::read(&dst)?, b"precious");
    // A temporary file left by a crashed run is also refused, not silently reused or deleted.
    let other = d.join("other.db");
    std::fs::write(d.join("other.db.partial"), b"stale")?;
    assert!(matches!(
        copy_verified(&src, &other, &key().public(), None),
        Err(CopyError::Exists(_))
    ));
    assert_eq!(std::fs::read(d.join("other.db.partial"))?, b"stale");
    // A dangling symlink at the destination is still "something there".
    let link = d.join("link.db");
    std::os::unix::fs::symlink(d.join("nowhere"), &link)?;
    assert!(matches!(
        copy_verified(&src, &link, &key().public(), None),
        Err(CopyError::Exists(_))
    ));
    Ok(())
}

#[test]
fn a_missing_source_or_directory_is_an_error_that_leaves_nothing() -> Res {
    let d = dir("missing")?;
    let err =
        copy_verified(&d.join("none.db"), &d.join("b.db"), &key().public(), None).unwrap_err();
    assert!(matches!(err, CopyError::SourceMissing(_)), "{err}");
    let (d2, src, _) = source("missing-dir", 2)?;
    let err = copy_verified(&src, &d2.join("no/such/dir/b.db"), &key().public(), None).unwrap_err();
    assert!(matches!(err, CopyError::Storage(_)), "{err}");
    assert!(leftovers(&d, "b.db").is_empty());
    assert!(leftovers(&d2, "b.db").is_empty());
    Ok(())
}

#[test]
fn a_copy_that_does_not_verify_is_removed_and_never_published() -> Res {
    let (d, src, head) = source("bad-copies", 6)?;

    // Edited by someone holding the file: the copy inherits the edit and fails naming the event.
    {
        let conn = Connection::open(&src)?;
        conn.execute_batch("DROP TRIGGER events_no_update;")?;
        conn.execute("UPDATE events SET payload = x'7b7d' WHERE seq = 4", [])?;
    }
    let err = copy_verified(&src, &d.join("b.db"), &key().public(), None).unwrap_err();
    assert!(
        matches!(
            &err,
            CopyError::Verify(VerifyError::Event {
                seq: 4,
                reason: Reason::HashMismatch
            })
        ),
        "{err}"
    );
    assert!(
        leftovers(&d, "b.db").is_empty(),
        "no copy and no temporary file: {:?}",
        leftovers(&d, "b.db")
    );

    // The wrong key fails at event 1.
    let other = LogKey::from_seed([6; 32]).public();
    let err = copy_verified(&src, &d.join("b.db"), &other, None).unwrap_err();
    assert!(
        matches!(err, CopyError::Verify(VerifyError::Event { seq: 1, .. })),
        "{err}"
    );

    // A recorded head that the log does not reach: events were removed from the end.
    let (d2, src2, head2) = source("short-copy", 6)?;
    {
        let conn = Connection::open(&src2)?;
        conn.execute_batch("DROP TRIGGER events_no_delete;")?;
        conn.execute("DELETE FROM events WHERE seq > 4", [])?;
    }
    let err = copy_verified(&src2, &d2.join("b.db"), &key().public(), Some(head2)).unwrap_err();
    assert!(
        matches!(err, CopyError::Verify(VerifyError::HeadMismatch { .. })),
        "{err}"
    );
    assert!(!d2.join("b.db").exists() && !d2.join("b.db.partial").exists());
    // Without the recorded head the truncated log is, as ADR-0018 says, a valid shorter chain.
    assert_eq!(
        copy_verified(&src2, &d2.join("b.db"), &key().public(), None)?.events,
        4
    );
    let _ = head;
    Ok(())
}

#[test]
fn a_copy_of_a_copy_is_the_same_log() -> Res {
    let (d, src, head) = source("chain-of-copies", 7)?;
    copy_verified(&src, &d.join("one.db"), &key().public(), Some(head))?;
    copy_verified(
        &d.join("one.db"),
        &d.join("two.db"),
        &key().public(),
        Some(head),
    )?;
    assert_eq!(rows(&d.join("two.db"))?, rows(&src)?);
    Ok(())
}
