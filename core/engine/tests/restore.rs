//! S1.3: restore onto a fresh machine reproduces state byte for byte (ADR-0021).

use std::path::{Path, PathBuf};

use hyalos_engine::{Decider, Engine, Ruling};
use hyalos_log::{Log, LogKey, copy_verified};
use hyalos_policy::{GateRequest, Outcome};
use hyalos_types::{
    Digest, Event, Gate, HoldReason, JobId, JobOutcome, Origin, ReasonCode, Resolution, Stakes,
    SubstrateName, TaskId, Timestamp,
};

type Res = Result<(), Box<dyn std::error::Error>>;

/// A short reason code for a decision; these tests are about structure, not the code.
fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

fn key() -> LogKey {
    LogKey::from_seed([3; 32])
}

fn machine(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("restore-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 * n)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

/// A busy engine: finished work, a task held by a timed-out gate, one running job and a resolved hold.
fn busy(path: &Path) -> Result<Engine, Box<dyn std::error::Error>> {
    let mut e =
        Engine::open(path, key())?.with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    let mut n = 0;
    let mut step = |e: &mut Engine, event: Event| -> Result<u64, Box<dyn std::error::Error>> {
        n += 1;
        // A hold is released through the call that records who decided.
        if let Event::HoldResolved { hold, reason, .. } = event {
            return Ok(e.decide_hold(at(10 * n), hold, Decider::Human, reason)?.seq);
        }
        Ok(e.submit(at(10 * n), Origin::Engine, event)?.seq)
    };
    let done = TaskId::from_seq(step(&mut e, Event::TaskCreated { request: d(1) })?);
    let job = JobId::from_seq(step(
        &mut e,
        Event::JobStarted {
            task: done,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    step(
        &mut e,
        Event::JobEnded {
            job,
            outcome: JobOutcome::Succeeded,
            reason: why(),
        },
    )?;
    step(&mut e, Event::TaskCompleted { task: done })?;

    let held = TaskId::from_seq(step(&mut e, Event::TaskCreated { request: d(2) })?);
    let job = JobId::from_seq(step(
        &mut e,
        Event::JobStarted {
            task: held,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    let request = GateRequest {
        gate: Gate::Egress,
        inputs: d(9),
        stakes: Stakes::irreversible(),
    };
    let ruling = e.rule_gate(
        at(65),
        Origin::Broker,
        job,
        &request,
        Outcome::Hold {
            reason: ReasonCode::timed_out(),
            why: HoldReason::TimedOut,
        },
    )?;
    let Ruling::Held(hold) = ruling else {
        return Err("expected a hold".into());
    };
    step(
        &mut e,
        Event::HoldResolved {
            hold,
            by: Resolution::Human,
            reason: why(),
            signed: hyalos_types::Signed::Unrecorded,
        },
    )?;
    let job = JobId::from_seq(step(
        &mut e,
        Event::JobStarted {
            task: held,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
    )?);
    e.rule_gate(
        at(85),
        Origin::Broker,
        job,
        &request,
        Outcome::Refuse {
            reason: ReasonCode::timed_out(),
        },
    )?;

    let _later = TaskId::from_seq(step(&mut e, Event::TaskCreated { request: d(3) })?);
    Ok(e)
}

#[test]
fn restore_onto_a_fresh_machine_reproduces_state_byte_for_byte() -> Res {
    let old = machine("old-host")?;
    let mut live = busy(&old.join("events.db"))?;
    let head = live.head()?.ok_or("empty log")?;

    // Back up while the engine is still open, then restore on a machine that has nothing.
    let backup = old.join("backup.db");
    copy_verified(&old.join("events.db"), &backup, &key().public(), Some(head))?;
    let fresh = machine("new-host")?;
    let target = fresh.join("data").join("events.db");
    std::fs::create_dir_all(target.parent().ok_or("no parent")?)?;
    copy_verified(&backup, &target, &key().public(), Some(head))?;

    let mut restored = Engine::open(&target, key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    assert_eq!(restored.state(), live.state(), "the rebuilt state is equal");
    assert_eq!(
        restored.head()?,
        Some(head),
        "the chain ends at the same event with the same hash"
    );
    assert!(restored.state().invariant_violations().is_empty());
    assert_eq!(
        Log::open(&target, key())?.events()?,
        Log::open(&old.join("events.db"), key())?.events()?
    );

    // The same next event on both gives the same event hash and signature, which can only happen if
    // the stored history is identical, byte for byte.
    let next = Event::TaskCreated { request: d(4) };
    let a = live.submit(at(100), Origin::Engine, next.clone())?;
    let b = restored.submit(at(100), Origin::Engine, next)?;
    assert_eq!(a, b);
    assert_eq!(restored.state(), live.state());
    Ok(())
}

#[test]
fn a_restore_of_the_wrong_log_is_caught_by_the_recorded_head() -> Res {
    let host = machine("wrong-head")?;
    let live = busy(&host.join("events.db"))?;
    let recorded = live.head()?.ok_or("empty log")?;
    // More events happen after the head was recorded; a backup that stops short of it must not pass.
    let other = machine("short-backup")?;
    let mut shorter = Engine::open(&other.join("events.db"), key())?
        .with_operator(hyalos_types::OperatorKey::from_seed(&[0x0e; 32]));
    shorter.submit(at(1), Origin::Engine, Event::TaskCreated { request: d(1) })?;
    let result = copy_verified(
        &other.join("events.db"),
        &other.join("b.db"),
        &key().public(),
        Some(recorded),
    );
    assert!(result.is_err());
    assert!(!other.join("b.db").exists());
    Ok(())
}
