//! S5.9: a merge is approved by a human, bound to the head the operator reviewed, and a merge job's
//! manifest is minted only from that approval (ADR-0004, ADR-0012, ADR-0071).

use std::path::PathBuf;

use hyalos_engine::{
    Decider, Engine, EngineError, MintRefusal, RuleViolation, approvals, mint_codes,
};
use hyalos_log::{Head, LogKey};
use hyalos_types::provenance::Provenance;
use hyalos_types::{
    Approval, Attestation, Budget, CommitSha, CredentialTier, Digest, Event, JobId,
    MANIFEST_VERSION, Manifest, ManifestKey, MergeMethod, OperatorKey, Origin, ReasonCode,
    Resolution, Role, Signed, StepInputs, SubstrateName, TaskId, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const PERMIT_MERGE: &str = r#"permit (principal, action == Action::"merge", resource);"#;

fn key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn operator() -> OperatorKey {
    OperatorKey::from_seed(&[0x0e; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn why() -> ReasonCode {
    ReasonCode::new("merge.operator").unwrap_or_else(ReasonCode::unavailable)
}

fn head() -> Res<CommitSha> {
    Ok(CommitSha::new(HEAD).ok_or("sha")?)
}

fn path(test: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("merges-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn task(e: &mut Engine, n: u64) -> Res<TaskId> {
    let head = e.submit(
        at(n),
        Origin::Engine,
        Event::TaskCreated {
            request: Digest::from_bytes([n as u8; 32]),
        },
    )?;
    Ok(TaskId::from_seq(head.seq))
}

fn start(e: &mut Engine, n: u64, task: TaskId) -> Res<JobId> {
    let head = e.submit(
        at(n),
        Origin::Engine,
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("broker").ok_or("substrate")?,
            credentials: CredentialTier::BrokerHeld,
            attestation: Attestation::Unrecorded,
        },
    )?;
    Ok(JobId::from_seq(head.seq))
}

fn approve(e: &mut Engine, n: u64, task: TaskId) -> Result<Head, EngineError> {
    let head = CommitSha::new(HEAD).ok_or(EngineError::Poisoned)?;
    e.approve_merge(
        at(n),
        task,
        "octo/repo",
        42,
        head,
        MergeMethod::Squash,
        Decider::Human,
        why(),
    )
}

/// A change to a manifest, named.
type Case = (&'static str, Box<dyn Fn(&mut Manifest)>);

/// An engine whose policy permits merging, with task 2's merge approved at event 3 and its job 4
/// running.
fn approved(test: &str) -> Res<(Engine, JobId, u64)> {
    let mut e = Engine::open(&path(test)?, key())?.with_operator(operator());
    e.approve_policy(at(1), PERMIT_MERGE, Decider::Human, why())?;
    let t = task(&mut e, 2)?;
    let approval = approve(&mut e, 3, t)?.seq;
    let job = start(&mut e, 4, t)?;
    Ok((e, job, approval))
}

/// The manifest the merge command mints for `job`.
fn merge_manifest(job: JobId, approval: u64) -> Res<Manifest> {
    Ok(Manifest {
        version: MANIFEST_VERSION,
        job,
        role: Role::Deterministic,
        tainted: false,
        repo: "octo/repo".into(),
        branch_prefix: String::new(),
        pr_base: None,
        actions: ["merge".to_string()].into(),
        egress_hosts: Default::default(),
        tools: Default::default(),
        data_flows: Default::default(),
        budget: Budget { tokens: 0 },
        deadline: at(10_000),
        approval: Some(Approval {
            event: approval,
            pr: 42,
            head_sha: head()?,
            method: MergeMethod::Squash,
        }),
        handles: Default::default(),
        result: None,
    })
}

fn mint(e: &Engine, m: &Manifest) -> Result<hyalos_types::SignedManifest, MintRefusal> {
    e.mint_manifest(
        m,
        &StepInputs::of([Provenance::Operator]),
        &ManifestKey::from_seed([3; 32]),
        at(5),
    )
}

fn invalid(result: Result<hyalos_types::SignedManifest, MintRefusal>) -> Res<String> {
    match result {
        Err(MintRefusal::Invalid { reason }) => Ok(reason.as_str().to_string()),
        other => Err(format!("expected an invalid manifest, got {other:?}").into()),
    }
}

#[test]
fn an_approved_merge_is_minted_with_its_approval_and_nothing_else() -> Res {
    let (e, job, approval) = approved("minted")?;
    let m = merge_manifest(job, approval)?;
    let signed = mint(&e, &m)?;
    let opened = signed.open(&ManifestKey::from_seed([3; 32]).public())?;
    assert_eq!(opened.approval, m.approval);
    assert_eq!(
        e.state().merge_of(TaskId::from_seq(2)).map(|m| m.seq),
        Some(3)
    );
    Ok(())
}

#[test]
fn the_manifest_must_repeat_exactly_what_was_approved() -> Res {
    let (e, job, approval) = approved("exact")?;
    let other = CommitSha::new(&"f".repeat(40)).ok_or("sha")?;
    let cases: Vec<Case> = vec![
        (
            "another head",
            Box::new(move |m| {
                if let Some(a) = m.approval.as_mut() {
                    a.head_sha = other.clone();
                }
            }),
        ),
        (
            "another pull request",
            Box::new(|m| {
                if let Some(a) = m.approval.as_mut() {
                    a.pr = 43;
                }
            }),
        ),
        (
            "another method",
            Box::new(|m| {
                if let Some(a) = m.approval.as_mut() {
                    a.method = MergeMethod::Rebase;
                }
            }),
        ),
        (
            "another event",
            Box::new(|m| {
                if let Some(a) = m.approval.as_mut() {
                    a.event = 1;
                }
            }),
        ),
        ("another repo", Box::new(|m| m.repo = "octo/other".into())),
        ("no approval", Box::new(|m| m.approval = None)),
    ];
    for (case, change) in cases {
        let mut m = merge_manifest(job, approval)?;
        change(&mut m);
        assert_eq!(
            invalid(mint(&e, &m))?,
            mint_codes::merge_unapproved().as_str(),
            "{case}"
        );
    }
    Ok(())
}

#[test]
fn a_merge_job_is_untainted_and_does_nothing_else() -> Res {
    let (e, job, approval) = approved("alone")?;
    let cases: Vec<Case> = vec![
        ("tainted", Box::new(|m| m.tainted = true)),
        (
            "push too",
            Box::new(|m| {
                m.actions.insert("push".into());
            }),
        ),
        (
            "a tool",
            Box::new(|m| m.tools = ["Bash".to_string()].into()),
        ),
        (
            "a flow",
            Box::new(|m| m.data_flows = ["job->forge".to_string()].into()),
        ),
        (
            "a host",
            Box::new(|m| m.egress_hosts = ["example.com".to_string()].into()),
        ),
        ("a branch", Box::new(|m| m.branch_prefix = "hyalos/".into())),
        ("a base", Box::new(|m| m.pr_base = Some("main".into()))),
    ];
    for (case, change) in cases {
        let mut m = merge_manifest(job, approval)?;
        change(&mut m);
        assert_eq!(
            invalid(mint(&e, &m))?,
            mint_codes::merge_not_alone().as_str(),
            "{case}"
        );
    }
    // An approval on a manifest that does not merge stands in for nothing.
    let mut m = merge_manifest(job, approval)?;
    m.actions = ["read".to_string()].into();
    assert_eq!(
        invalid(mint(&e, &m))?,
        mint_codes::merge_unapproved().as_str()
    );
    Ok(())
}

#[test]
fn another_tasks_approval_does_not_mint_this_tasks_merge() -> Res {
    let (mut e, _, approval) = approved("other-task")?;
    let t = task(&mut e, 5)?;
    let job = start(&mut e, 6, t)?;
    assert_eq!(
        invalid(mint(&e, &merge_manifest(job, approval)?))?,
        mint_codes::merge_unapproved().as_str()
    );
    Ok(())
}

#[test]
fn the_policy_must_permit_merging_even_with_the_approval() -> Res {
    let mut e = Engine::open(&path("policy")?, key())?.with_operator(operator());
    e.approve_policy(
        at(1),
        r#"permit (principal, action == Action::"read", resource);"#,
        Decider::Human,
        why(),
    )?;
    let t = task(&mut e, 2)?;
    let approval = approve(&mut e, 3, t)?.seq;
    let job = start(&mut e, 4, t)?;
    match mint(&e, &merge_manifest(job, approval)?) {
        Err(MintRefusal::Denied { ask, .. }) => assert_eq!(ask, "merge"),
        other => return Err(format!("{other:?}").into()),
    }
    Ok(())
}

#[test]
fn only_a_human_approves_once_before_any_job_and_never_through_submit() -> Res {
    let mut e = Engine::open(&path("rules")?, key())?.with_operator(operator());
    let t = task(&mut e, 1)?;
    let head = head()?;
    for decider in [Decider::Policy, Decider::Verifier] {
        let refused = e.approve_merge(
            at(2),
            t,
            "octo/repo",
            42,
            head.clone(),
            MergeMethod::Merge,
            decider,
            why(),
        );
        assert!(
            matches!(
                refused,
                Err(EngineError::Rule(RuleViolation::MergeNeedsAHuman { .. }))
            ),
            "{refused:?}"
        );
    }
    for (repo, pr) in [("octo", 42), ("octo/repo", 0)] {
        let refused = e.approve_merge(
            at(2),
            t,
            repo,
            pr,
            head.clone(),
            MergeMethod::Merge,
            Decider::Human,
            why(),
        );
        assert!(
            matches!(
                refused,
                Err(EngineError::Rule(RuleViolation::MergeTargetInvalid))
            ),
            "{repo} {pr}: {refused:?}"
        );
    }
    approve(&mut e, 3, t)?;
    assert!(matches!(
        approve(&mut e, 4, t),
        Err(EngineError::Rule(RuleViolation::MergeAlreadyApproved(_)))
    ));
    // A task that has run a job has had its chance: the approval comes first.
    let other = task(&mut e, 5)?;
    start(&mut e, 6, other)?;
    assert!(matches!(
        approve(&mut e, 7, other),
        Err(EngineError::Rule(RuleViolation::MergeAlreadyApproved(_)))
    ));
    // Raw, it is refused whoever writes it, signed or not.
    let raw = Event::MergeApproved {
        task: task(&mut e, 8)?,
        repo: "octo/repo".into(),
        pr: 42,
        head,
        method: MergeMethod::Merge,
        by: Resolution::Human,
        reason: why(),
        signed: Signed::Unrecorded,
    };
    assert!(matches!(
        e.submit(at(9), Origin::Engine, raw.clone()),
        Err(EngineError::Rule(RuleViolation::ThroughTheEngineApi { .. }))
    ));
    assert!(matches!(
        e.submit(at(9), Origin::Broker, raw),
        Err(EngineError::Rule(RuleViolation::OriginNotAllowed { .. }))
    ));
    // Without the operator's key a human's approval is not recorded at all.
    let mut unsigned = Engine::open(&path("unsigned")?, key())?;
    let t = task(&mut unsigned, 1)?;
    assert!(matches!(
        approve(&mut unsigned, 2, t),
        Err(EngineError::NoOperatorKey)
    ));
    assert!(unsigned.state().merges().is_empty());
    Ok(())
}

#[test]
fn the_approval_is_signed_survives_compaction_and_verifies() -> Res {
    let p = path("compact")?;
    let (mut e, job, _) = {
        let mut e = Engine::open(&p, key())?.with_operator(operator());
        e.approve_policy(at(1), PERMIT_MERGE, Decider::Human, why())?;
        let t = task(&mut e, 2)?;
        let approval = approve(&mut e, 3, t)?.seq;
        let job = start(&mut e, 4, t)?;
        (e, job, approval)
    };
    // The operator signed it, and verify checks that signature like any human decision's.
    let checked = approvals(&p, &[], &key().public(), Some(&operator().public()))?;
    assert_eq!(checked.signed, vec![1, 3]);
    let archive = p.with_file_name("archive.db");
    e.compact(at(5), &archive)?;
    drop(e);
    let e = Engine::open(&p, key())?.with_operator(operator());
    let kept = e.state().merge_of(TaskId::from_seq(2)).ok_or("lost")?;
    assert_eq!(
        (kept.seq, kept.pr, kept.method),
        (3, 42, MergeMethod::Squash)
    );
    assert!(mint(&e, &merge_manifest(job, 3)?).is_ok());
    Ok(())
}

#[test]
fn an_approval_is_used_by_one_job_however_it_ended() -> Res {
    let (mut e, job, _) = approved("one-job")?;
    e.submit(
        at(5),
        Origin::Engine,
        Event::JobEnded {
            job,
            outcome: hyalos_types::JobOutcome::Failed,
            reason: why(),
        },
    )?;
    assert!(matches!(
        start(&mut e, 6, TaskId::from_seq(2)),
        Err(err) if err.to_string().contains("merge job")
    ));
    Ok(())
}
