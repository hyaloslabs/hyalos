//! S1.4: a log written under an older event schema loads under a newer engine with no manual step
//! (ADR-0023).
//!
//! The real version 1 is only one step behind, so these tests invent a longer history: version 1
//! events used other field names than today's, and a version 2 engine upcasts them (with the real
//! step where one exists). The logs are written raw,
//! exactly as an old build would have, then opened by the engine.

use std::path::{Path, PathBuf};

use hyalos_engine::{Decider, Engine, EngineError, replay_with};
use hyalos_log::{Log, LogKey, copy_verified, verify};
use hyalos_types::event::{SCHEMA, SCHEMA_VERSION};
use hyalos_types::{
    Answer, Digest, Event, EventError, Gate, HoldId, HoldReason, JobId, JobOutcome, Origin,
    ReasonCode, Schema, Stakes, SubstrateName, TaskId, Timestamp, Upcast, Value,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A short reason code for a decision; these tests are about structure, not the code.
fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

const SCHEMA_VERSION_PLUS_ONE: u32 = SCHEMA_VERSION + 1;

fn key() -> LogKey {
    LogKey::from_seed([3; 32])
}

fn path(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("versions-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 * n)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

// ---- the invented version 2 engine ---------------------------------------------------------------

fn rename(mut v: Value, from: &str, to: &str) -> Result<Value, String> {
    let object = v.as_object_mut().ok_or("not an object")?;
    let value = object
        .remove(from)
        .ok_or_else(|| format!("no field `{from}`"))?;
    object.insert(to.to_string(), value);
    Ok(v)
}

fn created(v: Value) -> Result<Value, String> {
    rename(v, "digest", "request")
}

fn started(v: Value) -> Result<Value, String> {
    // Today's job.started also names its substrate, credential tier and attestation (ADR-0043,
    // ADR-0050, ADR-0057); a
    // version 1 one did not.
    let mut v = rename(v, "task_id", "task")?;
    let object = v.as_object_mut().ok_or("not an object")?;
    object.insert("substrate".into(), Value::String("unrecorded".into()));
    object.insert("credentials".into(), Value::String("unrecorded".into()));
    object.insert("attestation".into(), Value::String("unrecorded".into()));
    Ok(v)
}

fn answered(mut v: Value) -> Result<Value, String> {
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("reason".into(), Value::String("legacy.unrecorded".into()));
    Ok(v)
}

/// Version 1 called the request `digest`, the job's task `task_id`, and did not record a reason (for
/// a gate's answer or a job's end) or a substrate. A version 1 hold had no stakes. Everything else is
/// unchanged, so `task.completed` and `hold.resolved` need no step.
fn opened(v: Value) -> Result<Value, String> {
    // The one change that really happened between the real versions 1 and 2 (ADR-0025): use the real
    // step, so this test cannot drift from what the build does.
    SCHEMA
        .upcast("hold.opened", 1, v)
        .map_err(|e| e.to_string())
}

fn ended(v: Value) -> Result<Value, String> {
    // The real step: today's job.ended says why (ADR-0053).
    SCHEMA.upcast("job.ended", 7, v).map_err(|e| e.to_string())
}

static V2: Schema = Schema {
    current: 2,
    steps: &[
        Upcast {
            kind: "job.ended",
            from: 1,
            step: ended,
        },
        Upcast {
            kind: "task.created",
            from: 1,
            step: created,
        },
        Upcast {
            kind: "job.started",
            from: 1,
            step: started,
        },
        Upcast {
            kind: "gate.answered",
            from: 1,
            step: answered,
        },
        Upcast {
            kind: "hold.opened",
            from: 1,
            step: opened,
        },
    ],
};

// ---- the old log, written as the old build wrote it ------------------------------------------------

fn raw(log: &mut Log, n: u64, kind: &str, version: u32, payload: &str) -> Res {
    log.append(at(n), Origin::Engine, kind, version, payload.as_bytes())?;
    Ok(())
}

/// Ten version 1 events: a finished task with an allowed gate answer, and a held task whose gate
/// answered unknown.
fn write_old_log(path: &Path) -> Res {
    let mut log = Log::open(path, key())?;
    let rows: [(&str, String); 10] = [
        (
            "task.created",
            format!(r#"{{"kind":"task.created","digest":"{}"}}"#, hex(1)),
        ),
        (
            "job.started",
            r#"{"kind":"job.started","task_id":1}"#.into(),
        ),
        (
            "gate.answered",
            format!(
                r#"{{"kind":"gate.answered","job":2,"gate":"egress","inputs":"{}","answer":"allow"}}"#,
                hex(9)
            ),
        ),
        (
            "job.ended",
            r#"{"kind":"job.ended","job":2,"outcome":"succeeded"}"#.into(),
        ),
        (
            "task.completed",
            r#"{"kind":"task.completed","task":1}"#.into(),
        ),
        (
            "task.created",
            format!(r#"{{"kind":"task.created","digest":"{}"}}"#, hex(2)),
        ),
        (
            "job.started",
            r#"{"kind":"job.started","task_id":6}"#.into(),
        ),
        (
            "gate.answered",
            format!(
                r#"{{"kind":"gate.answered","job":7,"gate":"content","inputs":"{}","answer":"unknown"}}"#,
                hex(8)
            ),
        ),
        (
            "job.ended",
            r#"{"kind":"job.ended","job":7,"outcome":"held"}"#.into(),
        ),
        (
            "hold.opened",
            format!(
                r#"{{"kind":"hold.opened","task":6,"gate":"content","inputs":"{}","reason":"unknown"}}"#,
                hex(8)
            ),
        ),
    ];
    for (i, (kind, payload)) in rows.iter().enumerate() {
        // An unknown answer, the job ending held and the hold opening are one write, at one time.
        raw(&mut log, (i as u64 + 1).min(8), kind, 1, payload)?;
    }
    Ok(())
}

/// The same history written by today's engine, which is what the upgraded engine must end up equal to.
fn reference_state(path: &Path) -> Res<hyalos_engine::State> {
    let mut e =
        Engine::open(path, key())?.with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let legacy = ReasonCode::new("legacy.unrecorded").ok_or("reason")?;
    let mut n = 0;
    let mut go = |e: &mut Engine, event: Event| -> Res {
        n += 1;
        e.submit(at(n), Origin::Engine, event)?;
        Ok(())
    };
    go(&mut e, Event::TaskCreated { request: d(1) })?;
    go(
        &mut e,
        Event::JobStarted {
            task: TaskId::from_seq(1),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?;
    go(
        &mut e,
        Event::GateAnswered {
            job: JobId::from_seq(2),
            gate: Gate::Egress,
            inputs: d(9),
            answer: Answer::Allow,
            reason: legacy.clone(),
        },
    )?;
    go(
        &mut e,
        Event::JobEnded {
            job: JobId::from_seq(2),
            outcome: JobOutcome::Succeeded,
            reason: legacy.clone(),
        },
    )?;
    go(
        &mut e,
        Event::TaskCompleted {
            task: TaskId::from_seq(1),
        },
    )?;
    go(&mut e, Event::TaskCreated { request: d(2) })?;
    go(
        &mut e,
        Event::JobStarted {
            task: TaskId::from_seq(6),
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?;
    e.submit_all(
        at(8),
        &[
            (
                Origin::Engine,
                Event::GateAnswered {
                    job: JobId::from_seq(7),
                    gate: Gate::Content,
                    inputs: d(8),
                    answer: Answer::Unknown,
                    reason: legacy.clone(),
                },
            ),
            (
                Origin::Engine,
                Event::JobEnded {
                    job: JobId::from_seq(7),
                    outcome: JobOutcome::Held,
                    reason: legacy,
                },
            ),
            (
                Origin::Engine,
                Event::HoldOpened {
                    task: TaskId::from_seq(6),
                    gate: Gate::Content,
                    inputs: d(8),
                    reason: HoldReason::Unknown,
                    stakes: Stakes::irreversible(),
                },
            ),
        ],
    )?;
    Ok(e.state().clone())
}

fn stored(path: &Path) -> Res<Vec<(u64, u32, Vec<u8>)>> {
    Ok(Log::open(path, key())?
        .events()?
        .into_iter()
        .map(|e| (e.seq, e.schema_version, e.payload))
        .collect())
}

#[test]
fn a_version_1_log_loads_under_a_version_2_engine_with_no_manual_step() -> Res {
    let old = path("loads")?;
    write_old_log(&old)?;
    let before = stored(&old)?;
    assert!(before.iter().all(|r| r.1 == 1));

    // Just open it: no migration command, no flag.
    let upgraded = Engine::open_with(&old, key(), &V2)?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));

    // The state is the one today's engine would have built from the same history...
    // (Today's engine also marks the first event written at the signed version, ADR-0070; an old log
    // has none, which is the one difference.)
    let image = |s: &hyalos_engine::State| s.to_image().map_err(|j| format!("{j:?}"));
    let mut want = image(&reference_state(&path("loads-ref")?)?)?;
    assert_eq!(want.signed_since, Some(1));
    want.signed_since = None;
    assert_eq!(image(upgraded.state())?, want);
    assert_eq!(upgraded.state().last_seq(), 10);
    // ...including the rule that an unknown answer holds the task.
    assert_eq!(upgraded.state().holds().len(), 1);

    // Nothing stored was touched: same rows, same version, same bytes, and it still verifies.
    drop(upgraded);
    assert_eq!(stored(&old)?, before);
    assert_eq!(verify(&old, &key().public(), None)?.events, 10);
    Ok(())
}

#[test]
fn new_events_are_written_at_the_new_version_and_the_mixed_log_keeps_working() -> Res {
    let old = path("mixed")?;
    write_old_log(&old)?;
    let before = stored(&old)?;

    let mut e = Engine::open_with(&old, key(), &V2)?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    // A hold written before stakes existed reads as the most cautious, so only a human releases it.
    let head = e.decide_hold(at(11), HoldId::from_seq(10), Decider::Human, why())?;
    let head2 = e.submit(at(12), Origin::Engine, Event::TaskCreated { request: d(3) })?;
    // The atomic write path stamps the version too.
    let head3 = e.submit_all(
        at(13),
        &[(
            Origin::Engine,
            Event::JobStarted {
                task: TaskId::from_seq(12),
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
        )],
    )?;
    let live = e.state().clone();
    drop(e);

    let rows = stored(&old)?;
    assert_eq!(
        &rows[..10],
        &before[..],
        "old rows are exactly as they were"
    );
    assert_eq!(
        rows[10..].iter().map(|r| r.1).collect::<Vec<_>>(),
        vec![2, 2, 2],
        "new rows carry the new version"
    );
    // The new task.created is in today's shape, the old ones in the old shape, in one log.
    assert!(String::from_utf8_lossy(&rows[10 + 1].2).contains(r#""request""#));
    assert!(String::from_utf8_lossy(&rows[0].2).contains(r#""digest""#));

    // The whole mixed chain verifies, with no knowledge of any schema, and against the recorded head.
    assert_eq!(
        verify(&old, &key().public(), Some(head3))?.events,
        13,
        "{head:?} {head2:?}"
    );

    // Reopening reproduces the live state, by the engine and by a bare replay.
    let reopened = Engine::open_with(&old, key(), &V2)?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    assert_eq!(*reopened.state(), live);
    assert_eq!(replay_with(&V2, &Log::open(&old, key())?.events()?)?, live);
    Ok(())
}

#[test]
fn a_mixed_log_survives_backup_and_restore() -> Res {
    let old = path("backup")?;
    write_old_log(&old)?;
    let mut e = Engine::open_with(&old, key(), &V2)?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let head = e.submit(at(11), Origin::Engine, Event::TaskCreated { request: d(3) })?;
    let live = e.state().clone();
    drop(e);

    let target = old.with_file_name("restored.db");
    copy_verified(&old, &target, &key().public(), Some(head))?;
    let restored = Engine::open_with(&target, key(), &V2)?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    assert_eq!(*restored.state(), live);
    Ok(())
}

#[test]
fn a_build_that_is_older_than_the_log_refuses_it_and_changes_nothing() -> Res {
    let p = path("downgrade")?;
    let mut e =
        Engine::open(&p, key())?.with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    e.submit(at(1), Origin::Engine, Event::TaskCreated { request: d(1) })?;
    drop(e);
    // A newer build appends an event at the next version.
    raw(
        &mut Log::open(&p, key())?,
        2,
        "task.created",
        SCHEMA_VERSION + 1,
        &format!(r#"{{"kind":"task.created","request":"{}"}}"#, hex(2)),
    )?;
    let before = stored(&p)?;

    // Today's build does not read that version.
    match Engine::open(&p, key()) {
        Err(EngineError::Event {
            seq: 2,
            error:
                EventError::UnsupportedVersion {
                    found: SCHEMA_VERSION_PLUS_ONE,
                    current: SCHEMA_VERSION,
                },
        }) => {}
        other => return Err(format!("expected a refusal naming event 2, got {other:?}").into()),
    }
    let message = Engine::open(&p, key()).unwrap_err().to_string();
    assert!(
        message.contains("event 2") && message.contains("newer Hyalos"),
        "{message}"
    );
    assert_eq!(stored(&p)?, before, "a refused open writes nothing");
    // It is still a perfectly good log for the build that wrote it.
    assert_eq!(verify(&p, &key().public(), None)?.events, 2);
    Ok(())
}

#[test]
fn an_old_event_that_cannot_be_upcast_stops_the_load_and_names_it() -> Res {
    let p = path("bad-old-event")?;
    let mut log = Log::open(&p, key())?;
    raw(&mut log, 1, "task.created", 1, r#"{"kind":"task.created"}"#)?;
    drop(log);
    match Engine::open_with(&p, key(), &V2) {
        Err(EngineError::Event {
            seq: 1,
            error: EventError::Upcast { from: 1, .. },
        }) => Ok(()),
        other => Err(format!("expected an upcast failure at event 1, got {other:?}").into()),
    }
}

#[test]
fn a_schema_table_that_contradicts_itself_is_refused_before_the_log_is_touched() -> Res {
    fn noop(v: Value) -> Result<Value, String> {
        Ok(v)
    }
    static BAD: Schema = Schema {
        current: 2,
        steps: &[Upcast {
            kind: "task.invented",
            from: 1,
            step: noop,
        }],
    };
    let p = path("bad-table")?;
    match Engine::open_with(&p, key(), &BAD) {
        Err(EngineError::Schema(reason)) => assert!(reason.contains("task.invented"), "{reason}"),
        other => return Err(format!("expected a schema error, got {other:?}").into()),
    }
    assert!(!p.exists(), "no log was created");
    Ok(())
}

#[test]
fn a_tampered_old_event_is_still_caught_before_any_upcasting() -> Res {
    // Verification runs on the stored bytes, so an upcast cannot launder a changed event.
    let old = path("tamper")?;
    write_old_log(&old)?;
    let mut bytes = std::fs::read(&old)?;
    let needle = hex(1);
    let i = bytes
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
        .ok_or("the first request digest is not in the file")?;
    bytes[i] = b'f';
    std::fs::write(&old, bytes)?;
    assert!(matches!(
        Engine::open_with(&old, key(), &V2),
        Err(EngineError::Verify(_))
    ));
    Ok(())
}
