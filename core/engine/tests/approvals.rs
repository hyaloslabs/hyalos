//! S5.4: every human decision carries the operator's own signature as well as the engine's (ADR-0003,
//! ADR-0012, ADR-0070). The engine's key alone cannot make one that `hyalos log verify` accepts.

use std::path::{Path, PathBuf};

use hyalos_engine::{
    ApprovalError, Decider, Engine, EngineError, RuleViolation, Unrecorded, approvals,
};
use hyalos_log::{Log, LogKey};
use hyalos_types::event::SCHEMA_VERSION;
use hyalos_types::{
    AgentId, AgentName, Digest, Event, OperatorKey, Origin, ReasonCode, Resolution, Signed,
    Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn operator() -> OperatorKey {
    OperatorKey::from_seed(&[0x0e; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn code() -> ReasonCode {
    ReasonCode::new("agent.issued").unwrap_or_else(ReasonCode::unavailable)
}

fn name(text: &str) -> Res<AgentName> {
    Ok(AgentName::new(text).ok_or("name")?)
}

fn path(test: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("approvals-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn issued(name: AgentName, byte: u8, signed: Signed) -> Event {
    Event::AgentIssued {
        name,
        credential: Digest::from_bytes([byte; 32]),
        by: Resolution::Human,
        reason: code(),
        signed,
    }
}

/// Two credentials issued by the operator, then the first revoked by policy.
fn signed_log(test: &str) -> Res<PathBuf> {
    let p = path(test)?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    e.issue_agent(
        at(1),
        name("one")?,
        Digest::from_bytes([1; 32]),
        Decider::Human,
        code(),
    )?;
    e.issue_agent(
        at(2),
        name("two")?,
        Digest::from_bytes([2; 32]),
        Decider::Human,
        code(),
    )?;
    e.revoke_agent(at(3), AgentId::from_seq(1), Decider::Policy, code())?;
    Ok(p)
}

fn stored(p: &Path) -> Res<Vec<(Event, [u8; 32])>> {
    let mut out = Vec::new();
    for e in Log::open(p, key())?.events()? {
        out.push((
            Event::decode(&e.kind, e.schema_version, &e.payload)?,
            e.hash,
        ));
    }
    Ok(out)
}

fn append(p: &Path, n: u64, version: u32, kind: &str, payload: &[u8]) -> Res {
    Log::open(p, key())?.append(at(n), Origin::Engine, kind, version, payload)?;
    Ok(())
}

#[test]
fn the_engine_signs_each_human_decision_for_its_place_in_this_log() -> Res {
    let p = signed_log("place")?;
    let events = stored(&p)?;
    let engine = key().public();
    for (i, (event, _)) in events.iter().enumerate().take(2) {
        let Some(Signed::Operator(approval)) = event.signed() else {
            return Err(format!("event {} is not signed", i + 1).into());
        };
        assert_eq!(approval.operator, operator().public());
        // Each names the event before it, with that event's hash.
        assert_eq!(approval.after, i as u64);
        let hash = if i == 0 { [0; 32] } else { events[i - 1].1 };
        assert_eq!(approval.after_hash, Digest::from_bytes(hash));
        assert!(approval.is_by(
            &operator().public(),
            engine.as_bytes(),
            &event.approval_bytes()?
        ));
    }
    // A policy decision carries none.
    assert_eq!(events[2].0.signed(), Some(&Signed::None));
    let found = approvals(&p, &[], &engine, Some(&operator().public()))?;
    assert_eq!(found.signed, vec![1, 2]);
    assert!(found.unrecorded.is_empty());
    // It reopens: replay accepts what the engine wrote.
    Engine::open(&p, key())?;
    Ok(())
}

#[test]
fn without_the_operators_key_a_human_decision_is_refused_and_nothing_is_written() -> Res {
    let p = path("no-key")?;
    let mut e = Engine::open(&p, key())?;
    let refused = e.issue_agent(
        at(1),
        name("one")?,
        Digest::from_bytes([1; 32]),
        Decider::Human,
        code(),
    );
    assert!(
        matches!(refused, Err(EngineError::NoOperatorKey)),
        "{refused:?}"
    );
    assert!(e.state().agents().is_empty());
    assert!(Log::open(&p, key())?.head()?.is_none());
    Ok(())
}

#[test]
fn an_approval_signed_with_any_other_key_such_as_the_agents_fails_verify() -> Res {
    // The agent holds only its bearer credential. A key made from it is still not the operator's.
    let credential = [0x5a; 32];
    let p = path("agent-key")?;
    let mut e = Engine::open(&p, key())?.with_operator(OperatorKey::from_seed(&credential));
    e.issue_agent(
        at(1),
        name("one")?,
        Digest::from_bytes([1; 32]),
        Decider::Human,
        code(),
    )?;
    drop(e);
    let refused = approvals(&p, &[], &key().public(), Some(&operator().public()));
    assert!(
        matches!(refused, Err(ApprovalError::WrongOperator { seq: 1 })),
        "{refused:?}"
    );
    Ok(())
}

#[test]
fn a_human_decision_the_engine_writes_without_the_operators_signature_fails() -> Res {
    // The engine's key alone appends it, signed and chained like any event.
    for (test, signed) in [("none", Signed::None), ("unrecorded", Signed::Unrecorded)] {
        let p = path(&format!("forged-{test}"))?;
        let event = issued(name("one")?, 1, signed.clone());
        append(&p, 1, SCHEMA_VERSION, event.kind(), &event.encode()?)?;
        let refused = approvals(&p, &[], &key().public(), Some(&operator().public()));
        assert!(
            matches!(refused, Err(ApprovalError::Unsigned { seq: 1 })),
            "{test}: {refused:?}"
        );
        // Replay refuses it too, so the engine, the MCP server and minting never act on it.
        let open = Engine::open(&p, key());
        assert!(
            matches!(
                open,
                Err(EngineError::Replay {
                    seq: 1,
                    violation: RuleViolation::HumanDecisionUnsigned
                })
            ),
            "{test}: {open:?}"
        );
    }
    Ok(())
}

#[test]
fn a_human_decision_stamped_with_an_older_version_after_the_log_moved_on_is_refused() -> Res {
    // The version is a column the writer chooses. Once the log has an event at version 13, a row
    // claiming version 12 does not excuse a missing signature.
    let p = signed_log("downgraded")?;
    let payload = r#"{"kind":"agent.issued","name":"three","credential":"0303030303030303030303030303030303030303030303030303030303030303","by":"human","reason":"agent.issued"}"#;
    append(&p, 4, 12, "agent.issued", payload.as_bytes())?;
    let refused = approvals(&p, &[], &key().public(), Some(&operator().public()));
    assert!(
        matches!(refused, Err(ApprovalError::Unsigned { seq: 4 })),
        "{refused:?}"
    );
    let open = Engine::open(&p, key());
    assert!(
        matches!(
            open,
            Err(EngineError::Replay {
                seq: 4,
                violation: RuleViolation::HumanDecisionUnsigned
            })
        ),
        "{open:?}"
    );
    Ok(())
}

#[test]
fn compacting_neither_hides_a_forged_decision_nor_forgets_the_log_moved_on() -> Res {
    // A key that is not the operator's signs a decision; the state cannot tell, so it compacts.
    let p = path("compacted")?;
    let dir = p.parent().ok_or("dir")?.to_path_buf();
    let mut e = Engine::open(&p, key())?.with_operator(OperatorKey::from_seed(&[0x5a; 32]));
    e.issue_agent(
        at(1),
        name("one")?,
        Digest::from_bytes([1; 32]),
        Decider::Human,
        code(),
    )?;
    let archive = dir.join("archive-1.db");
    e.compact(at(2), &archive)?;
    drop(e);
    // The live log alone no longer holds it: its history rests on the snapshot.
    let operator_public = operator().public();
    assert!(
        approvals(&p, &[], &key().public(), Some(&operator_public))?
            .signed
            .is_empty()
    );
    // Given the archive, the decision is checked and refused.
    let refused = approvals(&p, &[archive], &key().public(), Some(&operator_public));
    assert!(
        matches!(refused, Err(ApprovalError::WrongOperator { seq: 1 })),
        "{refused:?}"
    );
    // The snapshot carries that the log reached the signed version, so an old-version unsigned
    // decision after it is still refused.
    let payload = r#"{"kind":"agent.issued","name":"two","credential":"0202020202020202020202020202020202020202020202020202020202020202","by":"human","reason":"agent.issued"}"#;
    append(&p, 3, 12, "agent.issued", payload.as_bytes())?;
    let refused = approvals(&p, &[], &key().public(), Some(&operator_public));
    assert!(
        matches!(refused, Err(ApprovalError::Unsigned { .. })),
        "{refused:?}"
    );
    assert!(matches!(
        Engine::open(&p, key()),
        Err(EngineError::Replay {
            violation: RuleViolation::HumanDecisionUnsigned,
            ..
        })
    ));
    Ok(())
}

#[test]
fn a_human_decision_from_before_signatures_passes_and_is_listed() -> Res {
    let p = path("legacy")?;
    let payload = r#"{"kind":"agent.issued","name":"one","credential":"0101010101010101010101010101010101010101010101010101010101010101","by":"human","reason":"agent.issued"}"#;
    append(&p, 1, 12, "agent.issued", payload.as_bytes())?;
    // No operator key is needed for a log with no signed decision.
    let found = approvals(&p, &[], &key().public(), None)?;
    assert!(found.signed.is_empty());
    assert_eq!(
        found.unrecorded,
        vec![Unrecorded {
            seq: 1,
            kind: "agent.issued".into()
        }]
    );
    Engine::open(&p, key())?;
    Ok(())
}

#[test]
fn a_signed_log_needs_the_operators_key_to_pass() -> Res {
    let p = signed_log("needs-key")?;
    let refused = approvals(&p, &[], &key().public(), None);
    assert!(
        matches!(refused, Err(ApprovalError::NoOperatorKey { seq: 1 })),
        "{refused:?}"
    );
    Ok(())
}

#[test]
fn a_signature_moved_to_another_event_or_another_log_does_not_hold() -> Res {
    let p = signed_log("moved")?;
    let (first, _) = stored(&p)?.remove(0);
    let Some(Signed::Operator(approval)) = first.signed().cloned() else {
        return Err("not signed".into());
    };
    // The same signature on another credential.
    let other = path("moved-event")?;
    let changed = issued(name("one")?, 9, Signed::Operator(approval.clone()));
    append(
        &other,
        1,
        SCHEMA_VERSION,
        changed.kind(),
        &changed.encode()?,
    )?;
    let refused = approvals(&other, &[], &key().public(), Some(&operator().public()));
    assert!(
        matches!(refused, Err(ApprovalError::BadSignature { seq: 1 })),
        "{refused:?}"
    );
    // The same event in a log another engine key signs.
    let elsewhere = path("moved-log")?;
    let theirs = || LogKey::from_seed([3; 32]);
    Log::open(&elsewhere, theirs())?.append(
        at(1),
        Origin::Engine,
        first.kind(),
        SCHEMA_VERSION,
        &first.encode()?,
    )?;
    let refused = approvals(
        &elsewhere,
        &[],
        &theirs().public(),
        Some(&operator().public()),
    );
    assert!(
        matches!(refused, Err(ApprovalError::BadSignature { seq: 1 })),
        "{refused:?}"
    );
    Ok(())
}

#[test]
fn an_approval_signed_for_an_earlier_place_is_refused() -> Res {
    let p = signed_log("earlier")?;
    let engine = key().public();
    // The operator signed this one at the start of the log and it was held back until now, after two
    // later approvals: written now, it is out of place.
    let late = issued(name("three")?, 3, Signed::None);
    let approval = operator().approve(
        engine.as_bytes(),
        0,
        Digest::from_bytes([0; 32]),
        &late.approval_bytes()?,
    );
    let late = late.with_signed(Signed::Operator(approval));
    append(&p, 4, SCHEMA_VERSION, late.kind(), &late.encode()?)?;
    let refused = approvals(&p, &[], &engine, Some(&operator().public()));
    assert!(
        matches!(refused, Err(ApprovalError::WrongPlace { seq: 4, after: 0 })),
        "{refused:?}"
    );
    let open = Engine::open(&p, key());
    assert!(
        matches!(
            open,
            Err(EngineError::Replay {
                seq: 4,
                violation: RuleViolation::ApprovalOutOfPlace { .. }
            })
        ),
        "{open:?}"
    );
    Ok(())
}

#[test]
fn an_approval_that_names_another_history_is_refused() -> Res {
    let p = signed_log("history")?;
    let engine = key().public();
    // Signed after event 3, but with a hash that is not event 3's: the operator saw another log.
    let event = issued(name("three")?, 3, Signed::None);
    let approval = operator().approve(
        engine.as_bytes(),
        3,
        Digest::from_bytes([7; 32]),
        &event.approval_bytes()?,
    );
    let event = event.with_signed(Signed::Operator(approval));
    append(&p, 4, SCHEMA_VERSION, event.kind(), &event.encode()?)?;
    let refused = approvals(&p, &[], &engine, Some(&operator().public()));
    assert!(
        matches!(refused, Err(ApprovalError::WrongPlace { seq: 4, after: 3 })),
        "{refused:?}"
    );
    Ok(())
}

#[test]
fn only_a_human_decision_carries_a_signature() -> Res {
    let p = path("not-human")?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    e.issue_agent(
        at(1),
        name("one")?,
        Digest::from_bytes([1; 32]),
        Decider::Human,
        code(),
    )?;
    let engine = key().public();
    let revoke = Event::AgentRevoked {
        agent: AgentId::from_seq(1),
        by: Resolution::Policy,
        reason: code(),
        signed: Signed::None,
    };
    let approval = operator().approve(
        engine.as_bytes(),
        1,
        Digest::from_bytes([0; 32]),
        &revoke.approval_bytes()?,
    );
    let revoke = revoke.with_signed(Signed::Operator(approval));
    assert!(matches!(
        e.state().check(&revoke),
        Err(RuleViolation::SignedButNotHuman {
            by: Resolution::Policy
        })
    ));
    Ok(())
}
