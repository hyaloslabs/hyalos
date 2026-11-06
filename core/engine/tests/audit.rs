//! S1.15: a snapshot's state is checked against a replay of the archives, and an audit that cannot
//! reach a snapshot says so instead of passing (ADR-0026).

use std::path::{Path, PathBuf};

use hyalos_engine::{Audit, AuditError, Engine, Gap, audit};
use hyalos_log::{Head, Log, LogKey, Origin, VerifyError, verify_history};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_types::snapshot::{STATE_VERSION, StateImage};
use hyalos_types::{
    Digest, Event, Gate, HoldId, HoldReason, JobId, JobOutcome, ReasonCode, Stakes, SubstrateName,
    TaskId, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([5; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("audit-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

fn put(e: &mut Engine, n: u64, event: Event) -> Res {
    e.submit(at(n), Origin::Engine, event)?;
    Ok(())
}

/// Events 1 to 9: two tasks, a finished job, a completed task, and a job held on an unknown answer.
fn first_part(e: &mut Engine) -> Res {
    put(e, 1, Event::TaskCreated { request: d(1) })?;
    put(e, 2, Event::TaskCreated { request: d(2) })?;
    put(
        e,
        3,
        Event::JobStarted {
            task: TaskId::from_seq(1),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?;
    put(
        e,
        4,
        Event::JobEnded {
            job: JobId::from_seq(3),
            outcome: JobOutcome::Succeeded,
            reason: why(),
        },
    )?;
    put(
        e,
        5,
        Event::TaskCompleted {
            task: TaskId::from_seq(1),
        },
    )?;
    put(
        e,
        6,
        Event::JobStarted {
            task: TaskId::from_seq(2),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?;
    let request = GateRequest {
        gate: Gate::Egress,
        inputs: d(7),
        stakes: Stakes::routine(),
    };
    e.rule_gate(
        at(7),
        Origin::Broker,
        JobId::from_seq(6),
        &request,
        Outcome::Hold {
            reason: why(),
            why: HoldReason::Unknown,
        },
    )?;
    Ok(())
}

/// Events 10 to 12: the hold is decided, a task starts and a job runs on it.
fn second_part(e: &mut Engine) -> Res {
    e.decide_hold(
        at(10),
        HoldId::from_seq(9),
        hyalos_engine::Decider::Human,
        why(),
    )?;
    put(e, 11, Event::TaskCreated { request: d(11) })?;
    put(
        e,
        12,
        Event::JobStarted {
            task: TaskId::from_seq(11),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?;
    Ok(())
}

/// Event 13, which stays in the live log.
fn tail(e: &mut Engine) -> Res {
    put(e, 13, Event::TaskCreated { request: d(13) })
}

struct Built {
    live: PathBuf,
    a1: PathBuf,
    a2: PathBuf,
}

/// A log with two snapshots (at 9 and at 12) and one event after them, with both archives.
fn built(name: &str) -> Res<Built> {
    let d = dir(name)?;
    let live = d.join("events.db");
    let (a1, a2) = (d.join("a1.db"), d.join("a2.db"));
    let mut e = Engine::open(&live, key())?;
    first_part(&mut e)?;
    e.compact(at(20), &a1)?;
    second_part(&mut e)?;
    e.compact(at(21), &a2)?;
    tail(&mut e)?;
    Ok(Built { live, a1, a2 })
}

fn run(b: &Built, archives: &[&Path]) -> Result<Audit, AuditError> {
    let archives: Vec<PathBuf> = archives.iter().map(|p| p.to_path_buf()).collect();
    audit(&b.live, &archives, &key().public(), None)
}

#[test]
fn a_true_history_audits_clean_across_every_snapshot() -> Res {
    let b = built("clean")?;
    let result = run(&b, &[&b.a1, &b.a2])?;
    assert_eq!(
        result,
        Audit {
            snapshots: 2,
            audited: 2,
            replayed: 13,
            gap: None
        }
    );
    assert!(result.complete());
    // Order does not matter, and an archive given twice changes nothing.
    assert_eq!(run(&b, &[&b.a2, &b.a1, &b.a2])?, result);
    Ok(())
}

#[test]
fn archives_that_do_not_reach_a_snapshot_leave_it_unaudited_and_say_so() -> Res {
    let b = built("partial")?;
    // Nothing given: nothing could be compared, and that is not a pass.
    let none = run(&b, &[])?;
    assert_eq!(
        (none.snapshots, none.audited, none.replayed, none.gap),
        (
            2,
            0,
            0,
            Some(Gap {
                from: 1,
                through: 9
            })
        )
    );
    assert!(!none.complete());
    // The second archive alone does not start at event 1.
    let late = run(&b, &[&b.a2])?;
    assert_eq!(
        (late.audited, late.gap),
        (
            0,
            Some(Gap {
                from: 1,
                through: 9
            })
        )
    );
    // The first alone reaches the first snapshot only.
    let early = run(&b, &[&b.a1])?;
    assert_eq!(
        (early.audited, early.replayed, early.gap),
        (
            1,
            9,
            Some(Gap {
                from: 10,
                through: 12
            })
        )
    );
    assert!(!early.complete());
    Ok(())
}

#[test]
fn a_log_that_was_never_compacted_has_nothing_to_compare_but_must_still_replay() -> Res {
    let d = dir("never")?;
    let live = d.join("events.db");
    let mut e = Engine::open(&live, key())?;
    first_part(&mut e)?;
    let result = audit(&live, &[], &key().public(), None)?;
    assert_eq!(
        (result.snapshots, result.audited, result.replayed),
        (0, 0, 9)
    );
    assert!(result.complete());
    Ok(())
}

// ---- a snapshot the engine signed over a state its events do not give ----------------------------

/// The state the engine holds at the end of event 9, as an image.
fn truth_at_9(live: &Path) -> Res<StateImage> {
    let mut e = Engine::open(live, key())?;
    first_part(&mut e)?;
    Ok(e.state().to_image().map_err(|j| format!("pending {j}"))?)
}

/// A log whose snapshot at 9 carries `forge(truth)` instead of the truth, signed with the real key.
fn forged(name: &str, forge: impl Fn(&mut StateImage)) -> Res<Built> {
    let d = dir(name)?;
    let live = d.join("events.db");
    let (a1, a2) = (d.join("a1.db"), d.join("a2.db"));
    let mut image = truth_at_9(&live)?;
    forge(&mut image);
    let bytes = image.encode()?;
    let mut log = Log::open(&live, key())?;
    let head = log.archive(&a1)?;
    log.compact::<String>(head, at(20), |_, _| Ok((STATE_VERSION, bytes.clone())))?;
    Ok(Built { live, a1, a2 })
}

fn mismatch(result: Result<Audit, AuditError>) -> Res<(u64, String)> {
    match result {
        Err(AuditError::Mismatch { seq, difference }) => Ok((seq, difference)),
        other => Err(format!("expected a mismatch, got {other:?}").into()),
    }
}

#[test]
fn a_snapshot_over_a_wrong_state_is_named_with_its_first_differing_field() -> Res {
    type Forge = fn(&mut StateImage);
    let cases: Vec<(&str, Forge, &str)> = vec![
        (
            "completed",
            |i| i.tasks[0].completed_at = None,
            "state.tasks[id 1].completed_at: the replay has 1005, the snapshot has null",
        ),
        (
            "extra-task",
            |i| {
                let mut t = i.tasks[0].clone();
                t.id = TaskId::from_seq(99);
                i.tasks.push(t);
            },
            "state.tasks: the replay has 2 entries and the snapshot has 3",
        ),
        (
            "request",
            |i| i.tasks[1].request = d(0xee),
            "state.tasks[id 2].request: the replay has",
        ),
        (
            "hold-stakes",
            |i| i.holds[0].stakes = Stakes::irreversible(),
            "state.holds[id 9].stakes",
        ),
        (
            "job-running",
            |i| i.jobs[0].ended = None,
            "state.jobs[id 3].ended",
        ),
        (
            "pruned",
            |i| {
                i.pruned.push(hyalos_types::snapshot::PrunedImage {
                    seq: 9,
                    at: 1,
                    last_seq: 1,
                    last_hash: d(1),
                })
            },
            "state.pruned: the replay has 0 entries and the snapshot has 1",
        ),
        (
            "last-at",
            |i| i.last_at = Some(1),
            "state.last_at: the replay has",
        ),
    ];
    for (name, forge, expected) in cases {
        let b = forged(name, forge)?;
        // The signature and the chain are the engine's, so verify finds nothing wrong.
        verify_history(&b.live, std::slice::from_ref(&b.a1), &key().public(), None)?;
        let (seq, difference) = mismatch(run(&b, &[&b.a1]))?;
        assert_eq!(seq, 9, "{name}");
        assert!(difference.starts_with(expected), "{name}: {difference}");
        // Without the archive the same forgery goes unseen, and the audit says it did not look.
        let blind = run(&b, &[])?;
        assert_eq!((blind.audited, blind.complete()), (0, false), "{name}");
    }
    Ok(())
}

#[test]
fn the_first_wrong_snapshot_is_the_one_named_even_if_a_later_one_is_right() -> Res {
    // Snapshot 9 carries a wrong stamp. The engine carries on from it, and the next snapshot is taken
    // after more events have overwritten that stamp, so it is right. Only the first is named.
    let b = forged("first-wrong", |i| i.last_at = Some(5))?;
    let mut e = Engine::open(&b.live, key())?;
    second_part(&mut e)?;
    e.compact(at(21), &b.a2)?;
    drop(e);
    let (seq, difference) = mismatch(run(&b, &[&b.a1, &b.a2]))?;
    assert_eq!(seq, 9);
    assert!(difference.starts_with("state.last_at:"), "{difference}");
    Ok(())
}

#[test]
fn a_snapshot_that_cannot_be_read_is_an_error_not_a_skip() -> Res {
    let d = dir("unreadable")?;
    let live = d.join("events.db");
    let mut e = Engine::open(&live, key())?;
    first_part(&mut e)?;
    drop(e);
    for (name, version, bytes) in [
        ("garbage", STATE_VERSION, b"{}".to_vec()),
        ("future", STATE_VERSION + 1, b"{}".to_vec()),
    ] {
        let live = d.join(format!("{name}.db"));
        std::fs::copy(d.join("events.db"), &live)?;
        let archive = d.join(format!("{name}-archive.db"));
        let mut log = Log::open(&live, key())?;
        let head = log.archive(&archive)?;
        log.compact::<String>(head, at(20), |_, _| Ok((version, bytes.clone())))?;
        let result = audit(&live, &[archive], &key().public(), None);
        assert!(
            matches!(result, Err(AuditError::Snapshot { seq: 9, .. })),
            "{name}: {result:?}"
        );
    }
    // And a state that claims to end somewhere else than the snapshot says.
    let live2 = d.join("moved.db");
    std::fs::copy(&live, &live2)?;
    let mut image = truth_at_9(&d.join("scratch.db"))?;
    image.last_seq = 8;
    let bytes = image.encode()?;
    let archive = d.join("moved-archive.db");
    let mut log = Log::open(&live2, key())?;
    let head = log.archive(&archive)?;
    log.compact::<String>(head, at(20), |_, _| Ok((STATE_VERSION, bytes.clone())))?;
    let result = audit(&live2, &[archive], &key().public(), None);
    assert!(
        matches!(&result, Err(AuditError::Snapshot { seq: 9, reason }) if reason.contains("ends at event 8")),
        "{result:?}"
    );
    Ok(())
}

// ---- what is refused before any comparison -------------------------------------------------------

#[test]
fn another_key_another_logs_archive_and_a_wrong_head_are_refused() -> Res {
    let b = built("refused")?;
    let other = LogKey::from_seed([6; 32]).public();
    assert!(matches!(
        audit(&b.live, &[b.a1.clone(), b.a2.clone()], &other, None),
        Err(AuditError::Verify(_))
    ));

    // An archive of a different log does not end at one of this log's snapshots.
    let stranger = built("refused-stranger")?;
    let mut e = Engine::open(&stranger.live, key())?;
    put(&mut e, 14, Event::TaskCreated { request: d(14) })?;
    let foreign = stranger.a1.with_file_name("foreign.db");
    e.compact(at(30), &foreign)?;
    drop(e);
    let result = run(&b, &[&b.a1, &foreign]);
    assert!(
        matches!(
            result,
            Err(AuditError::Verify(VerifyError::ArchiveNotAnchored { .. }))
        ),
        "{result:?}"
    );

    // The head the operator recorded is honoured.
    let wrong = Head {
        seq: 99,
        hash: [1; 32],
    };
    let result = audit(&b.live, &[], &key().public(), Some(wrong));
    assert!(
        matches!(
            result,
            Err(AuditError::Verify(VerifyError::HeadMismatch { .. }))
        ),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn events_that_do_not_replay_are_reported_even_when_they_are_signed() -> Res {
    let d = dir("no-replay")?;
    let live = d.join("events.db");
    let mut log = Log::open(&live, key())?;
    log.append(
        at(1),
        Origin::Engine,
        "job.ended",
        hyalos_types::event::SCHEMA_VERSION,
        br#"{"kind":"job.ended","job":99,"outcome":"succeeded"}"#,
    )?;
    let result = audit(&live, &[], &key().public(), None);
    assert!(matches!(result, Err(AuditError::Replay(_))), "{result:?}");
    Ok(())
}

#[test]
fn an_audit_changes_nothing_whether_it_passes_or_fails() -> Res {
    let read = |b: &Built| -> Res<Vec<Vec<u8>>> {
        Ok(vec![
            std::fs::read(&b.live)?,
            std::fs::read(&b.a1)?,
            std::fs::read(&b.a2)?,
        ])
    };
    let good = built("read-only")?;
    let before = read(&good)?;
    run(&good, &[&good.a1, &good.a2])?;
    assert_eq!(read(&good)?, before);

    let bad = forged("read-only-forged", |i| i.last_at = Some(1))?;
    let before = vec![std::fs::read(&bad.live)?, std::fs::read(&bad.a1)?];
    mismatch(run(&bad, &[&bad.a1]))?;
    assert_eq!(
        vec![std::fs::read(&bad.live)?, std::fs::read(&bad.a1)?],
        before
    );
    Ok(())
}
