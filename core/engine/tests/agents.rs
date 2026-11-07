//! S5.8: a driving agent's credential, issued by a human and revoked by any decision (ADR-0010,
//! ADR-0012, ADR-0068). The log holds the credential's SHA-256, never the secret.

use std::path::{Path, PathBuf};

use hyalos_engine::{Decider, Engine, EngineError, RuleViolation, read_state, replay};
use hyalos_log::{Head, Log, LogKey};
use hyalos_types::{AgentId, AgentName, Digest, Event, Origin, ReasonCode, Resolution, Timestamp};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

fn name(text: &str) -> Res<AgentName> {
    Ok(AgentName::new(text).ok_or("name")?)
}

fn secret(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn engine(test: &str) -> Res<(Engine, PathBuf)> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("agents-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let e = Engine::open(&dir.join("events.db"), key())?;
    Ok((e, dir))
}

fn issue(e: &mut Engine, n: u64, who: &str, credential: u8) -> Res<AgentId> {
    let head = e.issue_agent(
        at(n),
        name(who)?,
        secret(credential),
        Decider::Human,
        code("agent.issued"),
    )?;
    Ok(AgentId::from_seq(head.seq))
}

fn violation(result: Result<Head, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

fn events(dir: &Path) -> Res<usize> {
    Ok(Log::open(&dir.join("events.db"), key())?.events()?.len())
}

#[test]
fn an_issued_credential_is_admitted_until_it_is_revoked() -> Res {
    let (mut e, _) = engine("admit")?;
    assert_eq!(e.state().admitted_agent(&secret(1)), None);
    let agent = issue(&mut e, 1, "claude-code", 1)?;
    assert_eq!(e.state().admitted_agent(&secret(1)), Some(agent));
    assert_eq!(e.state().admitted_agent(&secret(2)), None);

    e.revoke_agent(at(2), agent, Decider::Human, code("agent.revoked"))?;
    assert_eq!(e.state().admitted_agent(&secret(1)), None);
    let recorded = &e.state().agents()[&agent];
    assert_eq!(
        (recorded.by, recorded.revoked.as_ref().map(|r| r.by)),
        (Resolution::Human, Some(Resolution::Human))
    );
    Ok(())
}

#[test]
fn only_a_human_issues_a_credential_and_a_refusal_writes_nothing() -> Res {
    let (mut e, dir) = engine("human")?;
    let before = events(&dir)?;
    for decider in [Decider::Policy, Decider::Verifier] {
        let v = violation(e.issue_agent(
            at(1),
            name("claude-code")?,
            secret(1),
            decider,
            code("agent.issued"),
        ))?;
        assert!(matches!(v, RuleViolation::AgentNeedsAHuman { .. }), "{v}");
    }
    assert_eq!(events(&dir)?, before);
    assert!(e.state().agents().is_empty());
    Ok(())
}

#[test]
fn any_decision_revokes_but_only_once_and_only_a_known_agent() -> Res {
    let (mut e, _) = engine("revoke")?;
    let agent = issue(&mut e, 1, "claude-code", 1)?;
    let v = violation(e.revoke_agent(
        at(2),
        AgentId::from_seq(99),
        Decider::Human,
        code("agent.revoked"),
    ))?;
    assert!(matches!(v, RuleViolation::UnknownAgent(_)), "{v}");
    // A policy or a verifier may take a capability away, though only a human grants one.
    e.revoke_agent(at(3), agent, Decider::Policy, code("agent.revoked"))?;
    let v = violation(e.revoke_agent(at(4), agent, Decider::Human, code("agent.revoked")))?;
    assert!(matches!(v, RuleViolation::AgentAlreadyRevoked(_)), "{v}");
    Ok(())
}

#[test]
fn a_credential_is_never_issued_twice_and_a_name_in_force_is_not_shared() -> Res {
    let (mut e, _) = engine("unique")?;
    let first = issue(&mut e, 1, "claude-code", 1)?;
    let v = violation(e.issue_agent(
        at(2),
        name("hermes")?,
        secret(1),
        Decider::Human,
        code("agent.issued"),
    ))?;
    assert!(matches!(v, RuleViolation::AgentCredentialReused), "{v}");
    let v = violation(e.issue_agent(
        at(2),
        name("claude-code")?,
        secret(2),
        Decider::Human,
        code("agent.issued"),
    ))?;
    assert!(matches!(v, RuleViolation::AgentNameInUse(_)), "{v}");

    // Once revoked, the name is free again, but the old secret never comes back.
    e.revoke_agent(at(3), first, Decider::Human, code("agent.revoked"))?;
    let v = violation(e.issue_agent(
        at(4),
        name("claude-code")?,
        secret(1),
        Decider::Human,
        code("agent.issued"),
    ))?;
    assert!(matches!(v, RuleViolation::AgentCredentialReused), "{v}");
    let second = issue(&mut e, 5, "claude-code", 2)?;
    assert_ne!(first, second);
    assert_eq!(e.state().admitted_agent(&secret(2)), Some(second));
    Ok(())
}

#[test]
fn raw_agent_events_are_refused_from_the_engine_and_the_broker() -> Res {
    let (mut e, dir) = engine("raw")?;
    let agent = issue(&mut e, 1, "claude-code", 1)?;
    let before = events(&dir)?;
    for event in [
        Event::AgentIssued {
            name: name("hermes")?,
            credential: secret(2),
            by: Resolution::Human,
            reason: code("agent.issued"),
        },
        Event::AgentRevoked {
            agent,
            by: Resolution::Human,
            reason: code("agent.revoked"),
        },
    ] {
        let v = violation(e.submit(at(2), Origin::Engine, event.clone()))?;
        assert!(
            matches!(v, RuleViolation::ThroughTheEngineApi { .. }),
            "{v}"
        );
        let v = violation(e.submit(at(2), Origin::Broker, event))?;
        assert!(matches!(v, RuleViolation::OriginNotAllowed { .. }), "{v}");
    }
    assert_eq!(events(&dir)?, before);
    Ok(())
}

#[test]
fn agents_survive_replay_compaction_and_a_read_with_the_public_key_alone() -> Res {
    let (mut e, dir) = engine("compact")?;
    let revoked = issue(&mut e, 1, "claude-code", 1)?;
    e.revoke_agent(at(2), revoked, Decider::Human, code("agent.revoked"))?;
    let live = issue(&mut e, 3, "hermes", 2)?;
    let all = Log::open(&dir.join("events.db"), key())?.events()?;
    assert_eq!(replay(&all)?.agents(), e.state().agents());

    let before = e.state().agents().clone();
    e.compact(at(10), &dir.join("archive.db"))?;
    drop(e);
    let reopened = Engine::open(&dir.join("events.db"), key())?;
    assert_eq!(reopened.state().agents(), &before);
    drop(reopened);

    // This is what the MCP server reads: no private key.
    let state = read_state(&dir.join("events.db"), &key().public())?;
    assert_eq!(state.admitted_agent(&secret(2)), Some(live));
    assert_eq!(state.admitted_agent(&secret(1)), None);
    Ok(())
}
