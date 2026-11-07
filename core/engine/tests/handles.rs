//! S4.4: raw text reaches an actor only through a handle a human released, and the decision is bound
//! to the handle's digest (ADR-0005, ADR-0012, ADR-0063).

use std::collections::BTreeSet;
use std::path::PathBuf;

use hyalos_engine::{Decider, Engine, EngineError, MintRefusal, RuleViolation, mint_codes, replay};
use hyalos_log::{Head, Log, LogKey};
use hyalos_types::{
    Attestation, Budget, CredentialTier, Digest, Event, JobId, JobOutcome, Manifest, ManifestKey,
    Origin, ReasonCode, Resolution, Role, StepInputs, SubstrateName, TaskId, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const POLICY: &str = r#"
    permit (principal, action == Action::"push", resource) when { resource.repo == "o/r" };
    permit (principal, action == Action::"data_flow", resource);
"#;

fn key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn manifest_key() -> ManifestKey {
    ManifestKey::from_seed([5; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

fn handle(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("handles-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn task(e: &mut Engine, n: u64) -> Res<TaskId> {
    let seq = e
        .submit(
            at(n),
            Origin::Engine,
            Event::TaskCreated {
                request: Digest::from_bytes([1; 32]),
            },
        )?
        .seq;
    Ok(TaskId::from_seq(seq))
}

fn start(e: &mut Engine, task: TaskId, n: u64) -> Res<JobId> {
    let seq = e
        .submit(
            at(n),
            Origin::Engine,
            Event::JobStarted {
                task,
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: CredentialTier::BrokerHeld,
                attestation: Attestation::no_hardware_root(),
            },
        )?
        .seq;
    Ok(JobId::from_seq(seq))
}

/// An engine with the policy approved and one task, no job yet.
fn engine(name: &str) -> Res<(Engine, TaskId, PathBuf)> {
    let d = dir(name)?;
    let mut e = Engine::open(&d.join("events.db"), key())?
        .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    e.approve_policy(at(1), POLICY, Decider::Human, code("policy.approved"))?;
    let t = task(&mut e, 2)?;
    Ok((e, t, d))
}

fn manifest(job: JobId, handles: &[Digest]) -> Manifest {
    Manifest {
        version: 1,
        job,
        role: Role::Actor,
        tainted: true,
        repo: "o/r".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: BTreeSet::from(["push".to_string()]),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::from(["job->model".to_string()]),
        budget: Budget { tokens: 1_000 },
        deadline: Timestamp::from_millis(10_000_000),
        approval: None,
        handles: handles.iter().copied().collect(),
        result: None,
    }
}

fn mint(e: &Engine, m: &Manifest) -> Result<hyalos_types::SignedManifest, MintRefusal> {
    e.mint_manifest(m, &StepInputs::default(), &manifest_key(), at(50))
}

fn violation(result: Result<Head, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

fn events(d: &std::path::Path) -> Res<usize> {
    Ok(Log::open(&d.join("events.db"), key())?.events()?.len())
}

#[test]
fn a_human_releases_a_handle_and_only_then_may_a_manifest_name_it() -> Res {
    let (mut e, t, _) = engine("release")?;
    let job = start(&mut e, t, 3)?;
    let m = manifest(job, &[handle(7)]);
    assert!(
        matches!(mint(&e, &m), Err(MintRefusal::Invalid { reason }) if reason == mint_codes::handle_not_released()),
        "an unreleased handle is not minted"
    );
    // A handle may be released while a job of the task runs: the decision is about the task.
    e.release_handle(at(4), t, handle(7), Decider::Human, code("handle.operator"))?;
    let signed = mint(&e, &m)?;
    assert_eq!(signed.open(&manifest_key().public())?.handles, m.handles);
    // Bound to the digest: another handle is still refused.
    let other = manifest(job, &[handle(7), handle(8)]);
    assert!(matches!(
        mint(&e, &other),
        Err(MintRefusal::Invalid { reason }) if reason == mint_codes::handle_not_released()
    ));
    // The log holds the digest and nothing else about the text.
    let released = &e.state().handles()[0];
    assert_eq!(
        (released.task, released.handle, released.by),
        (t, handle(7), Resolution::Human)
    );
    Ok(())
}

#[test]
fn only_a_human_may_release_a_handle_and_a_refusal_writes_nothing() -> Res {
    let (mut e, t, d) = engine("human")?;
    let before = events(&d)?;
    for decider in [Decider::Policy, Decider::Verifier] {
        let v = violation(e.release_handle(at(4), t, handle(7), decider, code("handle.operator")))?;
        assert!(matches!(v, RuleViolation::HandleNeedsAHuman { .. }), "{v}");
    }
    assert_eq!(events(&d)?, before);
    assert!(e.state().handles().is_empty());
    Ok(())
}

#[test]
fn a_raw_handle_release_is_refused_from_the_engine_and_the_broker() -> Res {
    let (mut e, t, d) = engine("raw")?;
    let event = Event::HandleReleased {
        task: t,
        handle: handle(7),
        by: Resolution::Human,
        reason: code("handle.operator"),
        signed: hyalos_types::Signed::Unrecorded,
    };
    let before = events(&d)?;
    let v = violation(e.submit(at(4), Origin::Engine, event.clone()))?;
    assert!(
        matches!(v, RuleViolation::ThroughTheEngineApi { .. }),
        "{v}"
    );
    let v = violation(e.submit(at(4), Origin::Broker, event))?;
    assert!(matches!(v, RuleViolation::OriginNotAllowed { .. }), "{v}");
    assert_eq!(events(&d)?, before);
    Ok(())
}

#[test]
fn a_handle_released_to_another_task_does_not_mint_for_this_one() -> Res {
    let (mut e, t, _) = engine("other-task")?;
    let other = task(&mut e, 3)?;
    e.release_handle(
        at(4),
        other,
        handle(7),
        Decider::Human,
        code("handle.operator"),
    )?;
    let job = start(&mut e, t, 5)?;
    assert!(matches!(
        mint(&e, &manifest(job, &[handle(7)])),
        Err(MintRefusal::Invalid { reason }) if reason == mint_codes::handle_not_released()
    ));
    Ok(())
}

#[test]
fn a_manifest_that_names_a_handle_is_minted_only_tainted() -> Res {
    let (mut e, t, _) = engine("taint")?;
    e.release_handle(at(3), t, handle(7), Decider::Human, code("handle.operator"))?;
    let job = start(&mut e, t, 4)?;
    let mut m = manifest(job, &[handle(7)]);
    m.role = Role::Reader;
    m.actions.clear();
    m.tainted = false;
    assert!(matches!(
        mint(&e, &m),
        Err(MintRefusal::Invalid { reason }) if reason == mint_codes::taint_required()
    ));
    m.tainted = true;
    mint(&e, &m)?;
    Ok(())
}

#[test]
fn a_handle_is_released_once_and_never_to_a_finished_task() -> Res {
    let (mut e, t, _) = engine("once")?;
    e.release_handle(at(3), t, handle(7), Decider::Human, code("handle.operator"))?;
    let v =
        violation(e.release_handle(at(4), t, handle(7), Decider::Human, code("handle.operator")))?;
    assert!(
        matches!(v, RuleViolation::HandleAlreadyReleased { .. }),
        "{v}"
    );
    let job = start(&mut e, t, 5)?;
    e.submit(
        at(6),
        Origin::Engine,
        Event::JobEnded {
            job,
            outcome: JobOutcome::Succeeded,
            reason: code("step.answered"),
        },
    )?;
    e.submit(at(7), Origin::Engine, Event::TaskCompleted { task: t })?;
    let v =
        violation(e.release_handle(at(8), t, handle(8), Decider::Human, code("handle.operator")))?;
    assert!(matches!(v, RuleViolation::TaskCompleted(_)), "{v}");
    Ok(())
}

#[test]
fn released_handles_survive_replay_and_compaction() -> Res {
    let (mut e, t, d) = engine("compact")?;
    e.release_handle(at(3), t, handle(7), Decider::Human, code("handle.operator"))?;
    let all = Log::open(&d.join("events.db"), key())?.events()?;
    assert_eq!(replay(&all)?.handles(), e.state().handles());
    let before = e.state().clone();
    e.compact(at(10), &d.join("archive.db"))?;
    drop(e);
    let reopened = Engine::open(&d.join("events.db"), key())?
        .with_operator(hyalos_types::OperatorKey::from_seed([0x0e; 32]));
    assert_eq!(reopened.state().handles(), before.handles());
    let mut reopened = reopened;
    let job = start(&mut reopened, t, 11)?;
    mint(&reopened, &manifest(job, &[handle(7)]))?;
    Ok(())
}
