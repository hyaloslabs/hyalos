//! S5.4: the operator approves through a channel of their own (ADR-0010, ADR-0012, ADR-0070).
//! `hyalos approve <hold>` and `hyalos cancel <task>` sign with the operator's key, opened by a
//! passphrase; `hyalos log verify` checks every human decision against `operator.pub` and fails on
//! one the engine's key alone wrote.

#![allow(dead_code)]

mod common;

use common::{PASSPHRASE, Res, World, ok_cli};
use hyalos_engine::Engine;
use hyalos_log::Log;
use hyalos_types::event::SCHEMA_VERSION;
use hyalos_types::{
    Digest, Event, Gate, HoldId, HoldReason, Origin, ReasonCode, Resolution, Signed, Stakes,
    TaskId, Timestamp,
};

/// Two tasks, each held at irreversible stakes, written with the engine's key as a run would.
fn two_holds(world: &World) -> Res<[(TaskId, HoldId); 2]> {
    let key = hyalos::keys::load(&world.config.join("engine.key"))?;
    let mut e = Engine::open(&world.data.join("events.db"), key)?;
    e.stamp_forward();
    let mut held = || -> Res<(TaskId, HoldId)> {
        let at = Timestamp::from_millis(1_000);
        let task = TaskId::from_seq(
            e.submit(
                at,
                Origin::Engine,
                Event::TaskCreated {
                    request: Digest::from_bytes([1; 32]),
                },
            )?
            .seq,
        );
        let hold = HoldId::from_seq(
            e.submit(
                at,
                Origin::Engine,
                Event::HoldOpened {
                    task,
                    gate: Gate::Content,
                    inputs: Digest::from_bytes([2; 32]),
                    reason: HoldReason::Unknown,
                    stakes: Stakes::irreversible(),
                },
            )?
            .seq,
        );
        Ok((task, hold))
    };
    Ok([held()?, held()?])
}

fn head(world: &World) -> Res<Option<u64>> {
    let key = hyalos::keys::load(&world.config.join("engine.key"))?;
    Ok(Log::open(&world.data.join("events.db"), key)?
        .head()?
        .map(|h| h.seq))
}

#[test]
fn approve_and_cancel_are_signed_by_the_operator_and_verify_checks_them() -> Res {
    let world = World::new("approve-signed", None)?;
    let [(_, first), (second, _)] = two_holds(&world)?;
    let out = ok_cli(world.signed(&["approve", &first.seq().to_string()])?)?;
    assert!(out.contains("signed by you"), "{out}");
    ok_cli(world.signed(&[
        "cancel",
        &second.seq().to_string(),
        "--reason",
        "operator.declined",
    ])?)?;
    // The policy approval of the world, the release and the cancellation: all three signed.
    let out = ok_cli(world.cli(&["log", "verify"])?)?;
    assert!(
        out.contains("human decisions: 3 signed by the operator"),
        "{out}"
    );
    let shown = ok_cli(world.cli(&["operator", "show"])?)?;
    assert!(
        out.contains(shown.trim()),
        "verify names the key in operator.pub: {out}"
    );
    let events = world.events()?;
    let signed = events
        .iter()
        .filter(|e| matches!(e.signed(), Some(Signed::Operator(_))))
        .count();
    assert_eq!(signed, 3);
    Ok(())
}

#[test]
fn a_wrong_passphrase_or_the_agents_credential_decides_nothing() -> Res {
    let world = World::new("approve-wrong", None)?;
    let [(_, hold), _] = two_holds(&world)?;
    let credential = ok_cli(world.signed(&["agent", "issue", "claude-code"])?)?;
    let before = head(&world)?;
    for passphrase in ["not the operator's passphrase", credential.trim()] {
        let out = world.signed_with(&["approve", &hold.seq().to_string()], passphrase)?;
        assert_eq!(out.status.code(), Some(1));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("does not open"), "{err}");
    }
    assert_eq!(head(&world)?, before, "nothing was written");
    Ok(())
}

#[test]
fn an_approval_key_is_made_once_and_needs_a_long_passphrase() -> Res {
    let world = World::new("approve-init", None)?;
    let again = world.signed(&["operator", "init"])?;
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("never replaced"));
    std::fs::remove_file(world.config.join("operator.key"))?;
    std::fs::remove_file(world.config.join("operator.pub"))?;
    let short = world.signed_with(&["operator", "init"], "short")?;
    assert_eq!(short.status.code(), Some(1));
    assert!(!world.config.join("operator.key").exists());
    // Without a key, a human decision is refused before anything is asked or written.
    let before = head(&world)?;
    let out = world.signed(&["agent", "issue", "claude-code"])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("hyalos operator init"));
    assert_eq!(head(&world)?, before);
    ok_cli(world.signed_with(&["operator", "init"], PASSPHRASE)?)?;
    Ok(())
}

#[test]
fn a_human_decision_the_engine_writes_without_the_operator_fails_verify() -> Res {
    let world = World::new("approve-forged", None)?;
    let [(_, hold), _] = two_holds(&world)?;
    // The engine's key alone writes a release that claims a human made it: chained and signed by the
    // engine like any event.
    let key = hyalos::keys::load(&world.config.join("engine.key"))?;
    let mut log = Log::open(&world.data.join("events.db"), key)?;
    let forged = Event::HoldResolved {
        hold,
        by: Resolution::Human,
        reason: ReasonCode::new("approved.operator").ok_or("code")?,
        signed: Signed::Unrecorded,
    };
    log.append(
        Timestamp::from_millis(u64::MAX / 4),
        Origin::Engine,
        forged.kind(),
        SCHEMA_VERSION,
        &forged.encode()?,
    )?;
    drop(log);
    let out = world.cli(&["log", "verify"])?;
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("without the operator's signature"), "{err}");
    Ok(())
}

#[test]
fn verify_fails_on_signed_decisions_checked_against_another_operator_key() -> Res {
    let world = World::new("approve-other-key", None)?;
    let other = hyalos_types::OperatorKey::from_seed([3; 32])
        .public()
        .to_hex();
    let out = world.cli(&["log", "verify", "--operator-key", &other])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not the operator's"));
    Ok(())
}
