//! S1.14: pruning an archive is recorded in the log, by the engine, before the file goes (ADR-0026).

use std::path::PathBuf;

use hyalos_engine::{Engine, EngineError, RuleViolation, State, pruned_archives};
use hyalos_log::{Head, Log, LogKey};
use hyalos_types::snapshot::PrunedImage;
use hyalos_types::{Digest, Event, Origin, Schema, Timestamp};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([6; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("prune-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn task(e: &mut Engine, n: u64) -> Res {
    e.submit(
        at(n),
        Origin::Engine,
        Event::TaskCreated {
            request: Digest::from_bytes([n as u8; 32]),
        },
    )?;
    Ok(())
}

/// An engine with `n` tasks that has been compacted once. Returns where the snapshot stands.
fn compacted(dir: &std::path::Path, n: u64) -> Res<(Engine, Head)> {
    let mut e = Engine::open(&dir.join("events.db"), key())?;
    for i in 1..=n {
        task(&mut e, i)?;
    }
    let done = e.compact(at(n + 1), &dir.join("a1.db"))?;
    Ok((e, done.head))
}

fn refused(result: Result<Head, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

#[test]
fn a_prune_is_an_engine_event_that_names_the_archive_and_survives_a_restart() -> Res {
    let d = dir("recorded")?;
    let (mut e, snapshot) = compacted(&d, 3)?;
    let head = e.record_prune(at(10), snapshot)?;
    assert_eq!(head.seq, snapshot.seq + 1);

    let recorded = &e.state().pruned()[0];
    assert_eq!(
        (recorded.seq, recorded.at, recorded.last_seq),
        (head.seq, at(10), snapshot.seq)
    );
    assert_eq!(recorded.last_hash, Digest::from_bytes(snapshot.hash));
    assert!(e.is_pruned(snapshot));

    // It is an ordinary signed event of kind archive.pruned, written by the engine.
    let log = Log::open(&d.join("events.db"), key())?;
    let stored = log.events()?;
    let last = stored.last().ok_or("no events")?;
    assert_eq!(
        (last.kind.as_str(), last.origin),
        ("archive.pruned", Origin::Engine)
    );
    drop(log);

    let live = e.state().clone();
    drop(e);
    let again = Engine::open(&d.join("events.db"), key())?;
    assert_eq!(*again.state(), live);
    Ok(())
}

#[test]
fn only_an_archive_the_log_made_can_be_recorded_and_only_once() -> Res {
    let d = dir("refused")?;
    let (mut e, snapshot) = compacted(&d, 3)?;
    let before = e.head()?;

    // Right event, wrong hash; wrong event, right hash; and an event past the end.
    let wrong_hash = Head {
        hash: [0xee; 32],
        ..snapshot
    };
    let wrong_seq = Head {
        seq: snapshot.seq - 1,
        ..snapshot
    };
    for head in [wrong_hash, wrong_seq] {
        assert_eq!(
            refused(e.record_prune(at(10), head))?,
            RuleViolation::NotASnapshotOfThisLog { last_seq: head.seq }
        );
    }
    assert_eq!(e.head()?, before, "refusals write nothing");

    e.record_prune(at(10), snapshot)?;
    assert_eq!(
        refused(e.record_prune(at(11), snapshot))?,
        RuleViolation::AlreadyPruned {
            last_seq: snapshot.seq
        }
    );
    assert_eq!(e.state().pruned().len(), 1);

    // A log that was never compacted has no snapshot, so nothing it holds can be an archive.
    let mut fresh = Engine::open(&d.join("fresh.db"), key())?;
    task(&mut fresh, 1)?;
    let head = fresh.head()?.ok_or("no head")?;
    assert!(matches!(
        refused(fresh.record_prune(at(5), head))?,
        RuleViolation::NotASnapshotOfThisLog { .. }
    ));
    Ok(())
}

#[test]
fn a_prune_naming_an_event_the_log_has_not_reached_is_refused() -> Res {
    let d = dir("future")?;
    let mut e = Engine::open(&d.join("events.db"), key())?;
    task(&mut e, 1)?;
    for last_seq in [0, 2, 99] {
        let event = Event::ArchivePruned {
            last_seq,
            last_hash: Digest::from_bytes([1; 32]),
        };
        assert_eq!(
            refused(e.submit(at(5), Origin::Engine, event))?,
            RuleViolation::ArchiveBeyondLog {
                last_seq,
                log_end: 1
            }
        );
    }
    assert_eq!(e.head()?.map(|h| h.seq), Some(1));
    Ok(())
}

#[test]
fn the_broker_cannot_record_a_prune() -> Res {
    let d = dir("broker")?;
    let (mut e, snapshot) = compacted(&d, 2)?;
    let event = Event::ArchivePruned {
        last_seq: snapshot.seq,
        last_hash: Digest::from_bytes(snapshot.hash),
    };
    let result = e.submit(at(10), Origin::Broker, event);
    assert!(
        matches!(
            result,
            Err(EngineError::Rule(RuleViolation::OriginNotAllowed { .. }))
        ),
        "{result:?}"
    );
    assert!(e.state().pruned().is_empty());
    Ok(())
}

#[test]
fn the_record_goes_into_the_next_snapshot_and_a_never_compacted_twin_agrees() -> Res {
    let d = dir("twin")?;
    let (mut e, snapshot) = compacted(&d, 3)?;
    e.record_prune(at(10), snapshot)?;
    task(&mut e, 11)?;
    // Compacting again takes the prune event out of the live log and into the snapshot.
    e.compact(at(12), &d.join("a2.db"))?;
    assert_eq!(
        Log::open(&d.join("events.db"), key())?.events()?.len(),
        0,
        "the prune event itself was archived"
    );
    let live = e.state().clone();
    let head = e.head()?;
    drop(e);

    let restarted = Engine::open(&d.join("events.db"), key())?;
    assert_eq!(*restarted.state(), live);
    assert_eq!(restarted.state().pruned().len(), 1);

    // The twin was fed the same events and never compacted. Same state, same head.
    let mut twin = Engine::open(&d.join("twin.db"), key())?;
    for i in 1..=3 {
        task(&mut twin, i)?;
    }
    twin.submit(
        at(10),
        Origin::Engine,
        Event::ArchivePruned {
            last_seq: snapshot.seq,
            last_hash: Digest::from_bytes(snapshot.hash),
        },
    )?;
    task(&mut twin, 11)?;
    // The compaction added no event, so the twin's state matches apart from nothing at all.
    assert_eq!(*twin.state(), live);
    assert_eq!(twin.head()?, head);
    Ok(())
}

#[test]
fn anyone_with_the_public_key_can_list_what_was_pruned() -> Res {
    let d = dir("list")?;
    let (mut e, snapshot) = compacted(&d, 3)?;
    let public = key().public();
    assert_eq!(pruned_archives(&d.join("events.db"), &public)?, vec![]);

    e.record_prune(at(10), snapshot)?;
    let listed = pruned_archives(&d.join("events.db"), &public)?;
    assert_eq!(listed, e.state().pruned());

    // Still listed once the record has moved into the snapshot, and without replaying anything.
    task(&mut e, 11)?;
    e.compact(at(12), &d.join("a2.db"))?;
    assert_eq!(pruned_archives(&d.join("events.db"), &public)?, listed);

    // The list is only as good as the log: another key, or a log that does not check out, is refused.
    let other = LogKey::from_seed([7; 32]).public();
    assert!(matches!(
        pruned_archives(&d.join("events.db"), &other),
        Err(EngineError::Verify(_))
    ));
    Ok(())
}

#[test]
fn a_build_from_before_the_event_existed_refuses_a_log_that_records_a_prune() -> Res {
    static BEFORE: Schema = Schema {
        current: 2,
        steps: &[],
    };
    let d = dir("older")?;
    let (mut e, snapshot) = compacted(&d, 2)?;
    e.record_prune(at(10), snapshot)?;
    drop(e);

    let result = Engine::open_with(&d.join("events.db"), key(), &BEFORE);
    let message = match result {
        Err(e @ EngineError::Event { .. }) => e.to_string(),
        other => return Err(format!("expected a refusal, got {:?}", other.map(|_| ())).into()),
    };
    assert!(message.contains("newer Hyalos"), "{message}");
    Ok(())
}

#[test]
fn a_snapshot_whose_prune_list_makes_no_sense_is_not_started_from() -> Res {
    let d = dir("image")?;
    let (mut e, snapshot) = compacted(&d, 3)?;
    e.record_prune(at(10), snapshot)?;
    let image = e.state().to_image().map_err(|j| format!("pending {j}"))?;
    assert_eq!(State::from_image(image.clone())?, *e.state());

    let entry = image.pruned[0].clone();
    let twice = |extra: PrunedImage| {
        let mut bad = image.clone();
        bad.pruned.push(extra);
        State::from_image(bad)
    };
    // Recorded twice.
    let error = twice(PrunedImage {
        seq: entry.seq,
        ..entry.clone()
    })
    .unwrap_err();
    assert!(error.contains("pruned twice"), "{error}");
    // Names an archive that ends after the record that mentions it, and one recorded past the end.
    for (seq, last_seq) in [(entry.seq, entry.seq + 1), (image.last_seq + 1, 1)] {
        let error = twice(PrunedImage {
            seq,
            last_seq,
            last_hash: Digest::from_bytes([9; 32]),
            ..entry.clone()
        })
        .unwrap_err();
        assert!(
            error.contains("names an archive ending at event"),
            "{error}"
        );
    }
    Ok(())
}
