//! S3.13: the operator's policy is stored as an approved event, the policy in force is the projection,
//! and the engine mints a manifest only within it (ADR-0003, ADR-0008, ADR-0012, ADR-0034).

use std::collections::BTreeSet;
use std::path::PathBuf;

use hyalos_engine::{Decider, Engine, EngineError, MintRefusal, RuleViolation, mint_codes, replay};
use hyalos_log::{Head, Log, LogKey};
use hyalos_policy::{LoadError, codes};
use hyalos_types::{
    Budget, Digest, Event, JobId, JobOutcome, MAX_POLICY_DOCUMENT, Manifest, ManifestKey, Origin,
    ReasonCode, Resolution, Role, StepInputs, SubstrateName, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([8; 32])
}

fn manifest_key() -> ManifestKey {
    ManifestKey::from_seed([5; 32])
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("policy-store-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn why() -> ReasonCode {
    ReasonCode::new("policy.approved").unwrap_or_else(ReasonCode::unavailable)
}

/// A policy like the one an operator writes once: this repo, a few tools, GitHub, a budget cap.
const OPERATOR: &str = r#"
    permit (principal, action in [Action::"read", Action::"push", Action::"open_pr", Action::"comment"], resource)
      when { resource.repo == "hyaloslabs/hyalos" && principal.budget_tokens <= 200000 };
    permit (principal, action == Action::"use_tool", resource)
      when { ["Bash", "Edit", "Read"].contains(resource.name) };
    permit (principal, action == Action::"data_flow", resource)
      when { resource.name == "job->model" };
    permit (principal, action == Action::"request_host", resource)
      when { ["github.com", "api.github.com"].contains(resource.name) };
"#;

/// An engine with one running job, as a manifest is minted for a job that has started.
fn running(name: &str) -> Res<(Engine, JobId, PathBuf)> {
    let d = dir(name)?;
    let path = d.join("events.db");
    let mut e = Engine::open(&path, key())?;
    let task = e
        .submit(
            at(1),
            Origin::Engine,
            Event::TaskCreated {
                request: Digest::from_bytes([1; 32]),
            },
        )?
        .seq;
    let job = e
        .submit(
            at(2),
            Origin::Engine,
            Event::JobStarted {
                task: hyalos_types::TaskId::from_seq(task),
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
        )?
        .seq;
    Ok((e, JobId::from_seq(job), d))
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn manifest(job: JobId) -> Manifest {
    Manifest {
        version: 1,
        job,
        role: Role::Actor,
        // An actor reads its repository, so it is always tainted (ADR-0005).
        tainted: true,
        repo: "hyaloslabs/hyalos".into(),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: set(&["open_pr", "push"]),
        egress_hosts: set(&["api.github.com", "github.com"]),
        tools: set(&["Bash", "Edit", "Read"]),
        data_flows: set(&["job->model"]),
        budget: Budget { tokens: 200_000 },
        deadline: Timestamp::from_millis(10_000_000),
        approval: None,
        handles: Default::default(),
    }
}

/// A change to a good manifest, and the first thing policy should refuse in it.
type Case = (&'static str, Box<dyn Fn(&mut Manifest)>, &'static str);

fn refusal(result: Result<hyalos_types::SignedManifest, MintRefusal>) -> Res<MintRefusal> {
    match result {
        Err(r) => Ok(r),
        Ok(_) => Err("expected a refusal, but a manifest was minted".into()),
    }
}

fn violation(result: Result<Head, EngineError>) -> Res<RuleViolation> {
    match result {
        Err(EngineError::Rule(v)) => Ok(v),
        other => Err(format!("expected a refusal, got {other:?}").into()),
    }
}

// ---- storing a policy -----------------------------------------------------------------------

#[test]
fn a_human_approves_a_policy_and_it_becomes_the_policy_in_force() -> Res {
    let (mut e, _, d) = running("approve")?;
    assert!(e.state().policy().is_none());
    let head = e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let in_force = e.state().policy().ok_or("no policy in force")?;
    assert_eq!(in_force.seq, head.seq);
    assert_eq!(in_force.by, Resolution::Human);
    assert_eq!(in_force.document, OPERATOR);
    assert_eq!(in_force.at, at(10));
    // The document itself is in the log, as the event ADR-0003 describes.
    let stored = Log::open(&d.join("events.db"), key())?.events()?;
    let last = stored.last().ok_or("empty log")?;
    assert_eq!(last.kind, "policy.approved");
    assert!(String::from_utf8_lossy(&last.payload).contains("resource.repo"));
    // The latest approval is the one in force.
    let later = "permit (principal, action == Action::\"read\", resource);";
    e.approve_policy(at(11), later, Decider::Human, why())?;
    assert_eq!(e.state().policy().map(|p| p.document.as_str()), Some(later));
    Ok(())
}

#[test]
fn only_a_human_may_approve_a_policy_and_a_refusal_writes_nothing() -> Res {
    let (mut e, _, _) = running("who")?;
    let before = e.head()?;
    for decider in [Decider::Policy, Decider::Verifier] {
        let v = violation(e.approve_policy(at(10), OPERATOR, decider, why()))?;
        assert!(
            matches!(v, RuleViolation::PolicyNeedsAHuman { .. }),
            "{decider:?}"
        );
    }
    assert_eq!(e.head()?, before);
    assert!(e.state().policy().is_none());
    // Not by submitting the event itself either, whoever sends it.
    let raw = Event::PolicyApproved {
        document: OPERATOR.into(),
        by: Resolution::Human,
        reason: why(),
    };
    assert!(matches!(
        violation(e.submit(at(11), Origin::Engine, raw.clone()))?,
        RuleViolation::ThroughTheEngineApi {
            kind: "policy.approved"
        }
    ));
    assert!(matches!(
        violation(e.submit(at(11), Origin::Broker, raw))?,
        RuleViolation::OriginNotAllowed { .. }
    ));
    assert_eq!(e.head()?, before);
    Ok(())
}

#[test]
fn a_policy_that_fails_validation_is_rejected_before_anyone_is_asked_and_writes_nothing() -> Res {
    let (mut e, _, _) = running("invalid")?;
    let before = e.head()?;
    let bad = [
        ("not cedar", "permit (principal", "not valid Cedar"),
        (
            "an attribute the schema lacks",
            "permit (principal, action, resource) when { principal.admin };",
            "does not match the schema",
        ),
        (
            "an action the schema lacks",
            "permit (principal, action == Action::\"nuke\", resource);",
            "does not match the schema",
        ),
        (
            "a template",
            "permit (principal == ?principal, action, resource);",
            "templates",
        ),
    ];
    for (what, document, expected) in bad {
        // Asked first, so a human is never shown it.
        let checked = Engine::check_policy(document);
        assert!(checked.is_err(), "{what}");
        let refused = e.approve_policy(at(10), document, Decider::Human, why());
        match refused {
            Err(EngineError::Policy(error)) => {
                assert!(error.to_string().contains(expected), "{what}: {error}");
            }
            other => return Err(format!("{what}: expected a refusal, got {other:?}").into()),
        }
    }
    assert_eq!(e.head()?, before);
    assert!(e.state().policy().is_none());
    assert!(matches!(
        Engine::check_policy("permit (principal"),
        Err(LoadError::Parse(_))
    ));
    Ok(())
}

#[test]
fn a_document_is_bounded() -> Res {
    let (mut e, _, _) = running("bounded")?;
    let base = "permit (principal, action == Action::\"read\", resource);\n//";
    let padded = |len: usize| format!("{base}{}", "a".repeat(len - base.len()));
    e.approve_policy(at(10), &padded(MAX_POLICY_DOCUMENT), Decider::Human, why())?;
    let v = violation(e.approve_policy(
        at(11),
        &padded(MAX_POLICY_DOCUMENT + 1),
        Decider::Human,
        why(),
    ))?;
    assert!(matches!(v, RuleViolation::PolicyTooLarge { len } if len == MAX_POLICY_DOCUMENT + 1));
    Ok(())
}

#[test]
fn the_policy_in_force_is_the_projection_of_the_log() -> Res {
    let (mut e, _, d) = running("replay")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    e.approve_policy(
        at(11),
        "permit (principal, action == Action::\"read\", resource);",
        Decider::Human,
        why(),
    )?;
    let live = e.state().clone();
    let path = d.join("events.db");
    drop(e);
    // Reopening replays the log; so does replaying the stored events directly.
    let reopened = Engine::open(&path, key())?;
    assert_eq!(reopened.state(), &live);
    let events = Log::open(&path, key())?.events()?;
    assert_eq!(&replay(&events)?, &live);
    assert_eq!(
        reopened.state().policy().map(|p| p.seq),
        live.policy().map(|p| p.seq)
    );
    Ok(())
}

#[test]
fn compaction_keeps_the_policy_in_force() -> Res {
    let (mut e, job, d) = running("compact")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let before = e.state().clone();
    e.compact(at(20), &d.join("archive.db"))?;
    let path = d.join("events.db");
    drop(e);
    let reopened = Engine::open(&path, key())?;
    assert_eq!(reopened.state().policy(), before.policy());
    // And it still mints from the policy it started from.
    assert!(
        reopened
            .mint_manifest(
                &manifest(job),
                &StepInputs::default(),
                &manifest_key(),
                at(30)
            )
            .is_ok()
    );
    Ok(())
}

// ---- minting within the policy -----------------------------------------------------------------

#[test]
fn a_manifest_within_policy_is_minted_signed_and_opens_with_the_public_key() -> Res {
    let (mut e, job, _) = running("mint")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let signed = e.mint_manifest(
        &manifest(job),
        &StepInputs::default(),
        &manifest_key(),
        at(20),
    )?;
    assert_eq!(signed.open(&manifest_key().public())?, manifest(job));
    // Another key does not open it.
    assert!(
        signed
            .open(&ManifestKey::from_seed([6; 32]).public())
            .is_err()
    );
    Ok(())
}

#[test]
fn a_job_request_outside_policy_is_denied_naming_what_was_refused() -> Res {
    let (mut e, job, _) = running("deny")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let key = manifest_key();
    let cases: Vec<Case> = vec![
        (
            "a tool not in policy",
            Box::new(|m| {
                m.tools.insert("Curl".into());
            }),
            "use_tool:Curl",
        ),
        (
            "a host not in policy",
            Box::new(|m| {
                m.egress_hosts.insert("evil.example".into());
            }),
            "request_host:evil.example",
        ),
        (
            "a flow not in policy",
            Box::new(|m| {
                m.data_flows.insert("job->web".into());
            }),
            "data_flow:job->web",
        ),
        (
            "a budget past the cap",
            Box::new(|m| m.budget.tokens = 200_001),
            "open_pr",
        ),
        (
            "another repo",
            Box::new(|m| m.repo = "someone/else".into()),
            "open_pr",
        ),
        (
            "an action policy does not permit",
            Box::new(|m| {
                m.actions.insert("publish".into());
            }),
            "publish",
        ),
    ];
    for (what, change, ask) in cases {
        let mut m = manifest(job);
        change(&mut m);
        match refusal(e.mint_manifest(&m, &StepInputs::default(), &key, at(20)))? {
            MintRefusal::Denied { ask: got, reason } => {
                assert!(
                    got == ask || got.starts_with(ask) || ask.starts_with(&got),
                    "{what}: {got}"
                );
                assert_eq!(reason, codes::denied(), "{what}");
            }
            other => return Err(format!("{what}: {other:?}").into()),
        }
    }
    Ok(())
}

#[test]
fn with_no_operator_policy_nothing_is_minted_that_asks_for_anything() -> Res {
    let (e, job, _) = running("nopolicy")?;
    assert!(matches!(
        refusal(e.mint_manifest(
            &manifest(job),
            &StepInputs::default(),
            &manifest_key(),
            at(20)
        ))?,
        MintRefusal::Denied { .. }
    ));
    // A manifest that asks for nothing is not asking for anything outside policy.
    let mut empty = manifest(job);
    empty.actions.clear();
    empty.tools.clear();
    empty.egress_hosts.clear();
    empty.data_flows.clear();
    assert!(
        e.mint_manifest(&empty, &StepInputs::default(), &manifest_key(), at(20))
            .is_ok()
    );
    Ok(())
}

#[test]
fn the_baseline_holds_whatever_the_operator_permits() -> Res {
    let (mut e, job, _) = running("baseline")?;
    e.approve_policy(
        at(10),
        "permit (principal, action, resource);",
        Decider::Human,
        why(),
    )?;
    // Irreversible actions need an approval, and no approval event exists yet to set the fact.
    for action in [
        "merge",
        "publish",
        "change_policy",
        "spend_over_cap",
        "export_credentials",
    ] {
        let mut m = manifest(job);
        m.actions = set(&[action]);
        m.approval = Some(hyalos_types::Approval {
            head_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            event: 5,
        });
        match refusal(e.mint_manifest(&m, &StepInputs::default(), &manifest_key(), at(20)))? {
            MintRefusal::Denied { reason, .. } => assert_eq!(reason, codes::denied(), "{action}"),
            other => return Err(format!("{action}: {other:?}").into()),
        }
    }
    // And a tainted job cannot merge, with or without anything else.
    let mut tainted = manifest(job);
    tainted.tainted = true;
    tainted.actions = set(&["merge"]);
    assert!(matches!(
        refusal(e.mint_manifest(&tainted, &StepInputs::default(), &manifest_key(), at(20)))?,
        MintRefusal::Denied { .. }
    ));
    // Under a permit-all, an ordinary manifest is minted.
    assert!(
        e.mint_manifest(
            &manifest(job),
            &StepInputs::default(),
            &manifest_key(),
            at(20)
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn a_manifest_the_engine_does_not_mint_is_refused_whatever_policy_says() -> Res {
    let (mut e, job, _) = running("invalid-manifest")?;
    e.approve_policy(
        at(10),
        "permit (principal, action, resource);",
        Decider::Human,
        why(),
    )?;
    let key = manifest_key();
    for action in ["nuke", "", "use_tool", "request_host", "data_flow", "PUSH"] {
        let mut m = manifest(job);
        m.actions = set(&[action]);
        assert_eq!(
            refusal(e.mint_manifest(&m, &StepInputs::default(), &key, at(20)))?,
            MintRefusal::Invalid {
                reason: mint_codes::unknown_action()
            },
            "{action:?}"
        );
    }
    // A data flow is one this build knows (ADR-0055), whatever the policy permits.
    for name in ["repo->model", "job->world", "", "JOB->MODEL"] {
        let mut m = manifest(job);
        m.data_flows.insert(name.into());
        assert_eq!(
            refusal(e.mint_manifest(&m, &StepInputs::default(), &key, at(20)))?,
            MintRefusal::Invalid {
                reason: mint_codes::unknown_flow()
            },
            "{name:?}"
        );
    }
    // A job that never started, and one that ended.
    let other = manifest(JobId::from_seq(999));
    assert_eq!(
        refusal(e.mint_manifest(&other, &StepInputs::default(), &key, at(20)))?,
        MintRefusal::Invalid {
            reason: mint_codes::job_not_running()
        }
    );
    e.submit(
        at(30),
        Origin::Engine,
        Event::JobEnded {
            job,
            outcome: JobOutcome::Succeeded,
            reason: why(),
        },
    )?;
    assert_eq!(
        refusal(e.mint_manifest(&manifest(job), &StepInputs::default(), &key, at(40)))?,
        MintRefusal::Invalid {
            reason: mint_codes::job_not_running()
        }
    );
    Ok(())
}

#[test]
fn a_deadline_that_is_not_in_the_future_is_refused() -> Res {
    let (mut e, job, _) = running("deadline")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let mut m = manifest(job);
    m.deadline = at(100);
    assert!(
        e.mint_manifest(&m, &StepInputs::default(), &manifest_key(), at(99))
            .is_ok()
    );
    for now in [100, 101, 5_000] {
        assert_eq!(
            refusal(e.mint_manifest(&m, &StepInputs::default(), &manifest_key(), at(now)))?,
            MintRefusal::Invalid {
                reason: mint_codes::expired()
            },
            "{now}"
        );
    }
    Ok(())
}

#[test]
fn a_policy_that_errors_holds_the_request_and_never_allows_it() -> Res {
    let (mut e, job, _) = running("errors")?;
    // Valid against the schema, and overflows when it runs, which Cedar reports as an error.
    e.approve_policy(
        at(10),
        "permit (principal, action, resource) when { principal.budget_tokens * 9223372036854775807 > 0 };",
        Decider::Human,
        why(),
    )?;
    let mut m = manifest(job);
    m.budget.tokens = 2;
    match refusal(e.mint_manifest(&m, &StepInputs::default(), &manifest_key(), at(20)))? {
        MintRefusal::Held { reason, .. } => assert_eq!(reason, codes::error()),
        other => return Err(format!("expected a hold, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn a_stored_policy_that_no_longer_validates_holds_requests_until_it_is_replaced() -> Res {
    // The log holds a policy that an older schema accepted and this one does not: written straight
    // into the log, as an upgrade would leave it.
    let d = dir("stale")?;
    let path = d.join("events.db");
    {
        let mut log = Log::open(&path, key())?;
        let events = [
            Event::TaskCreated {
                request: Digest::from_bytes([1; 32]),
            },
            Event::JobStarted {
                task: hyalos_types::TaskId::from_seq(1),
                substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
                credentials: hyalos_types::CredentialTier::BrokerHeld,
                attestation: hyalos_types::Attestation::no_hardware_root(),
            },
            Event::PolicyApproved {
                document: "permit (principal, action, resource) when { principal.admin };".into(),
                by: Resolution::Human,
                reason: why(),
            },
        ];
        for (i, event) in events.iter().enumerate() {
            log.append(
                at(i as u64 + 1),
                Origin::Engine,
                event.kind(),
                hyalos_types::event::SCHEMA_VERSION,
                &event.encode()?,
            )?;
        }
    }
    // Opening does not fail: the log is intact, and refusing to open would lock the operator out of
    // replacing the policy.
    let mut e = Engine::open(&path, key())?;
    let job = JobId::from_seq(2);
    let unavailable = e.policies().err().ok_or("the stale policy loaded")?;
    assert_eq!(unavailable.since, Some(3));
    assert!(unavailable.to_string().contains("held until a replacement"));
    match refusal(e.mint_manifest(
        &manifest(job),
        &StepInputs::default(),
        &manifest_key(),
        at(20),
    ))? {
        MintRefusal::Held { reason, .. } => assert_eq!(reason, codes::invalid()),
        other => return Err(format!("expected a hold, got {other:?}").into()),
    }
    // Even an ask that would be allowed by anything else is held, and nothing is minted.
    let mut nothing = manifest(job);
    nothing.actions = set(&["read"]);
    nothing.tools.clear();
    nothing.egress_hosts.clear();
    nothing.data_flows.clear();
    assert!(matches!(
        refusal(e.mint_manifest(&nothing, &StepInputs::default(), &manifest_key(), at(20)))?,
        MintRefusal::Held { .. }
    ));
    // A stale approval is replaced by approving a valid one, and then minting works.
    e.approve_policy(at(21), OPERATOR, Decider::Human, why())?;
    assert!(
        e.mint_manifest(
            &manifest(job),
            &StepInputs::default(),
            &manifest_key(),
            at(22)
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn minting_writes_nothing_to_the_log() -> Res {
    let (mut e, job, _) = running("pure")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let before = e.head()?;
    let state = e.state().clone();
    e.mint_manifest(
        &manifest(job),
        &StepInputs::default(),
        &manifest_key(),
        at(20),
    )?;
    let mut denied = manifest(job);
    denied.tools.insert("Curl".into());
    refusal(e.mint_manifest(&denied, &StepInputs::default(), &manifest_key(), at(20)))?;
    assert_eq!(e.head()?, before);
    assert_eq!(e.state(), &state);
    Ok(())
}

/// S3.14 (ADR-0005, ADR-0047): taint is derived from the step, never requested.
#[test]
fn an_actor_or_a_step_given_forge_content_is_minted_only_tainted() -> Res {
    let (mut e, job, _) = running("derived-taint")?;
    e.approve_policy(at(10), OPERATOR, Decider::Human, why())?;
    let required = |result| -> Res {
        match refusal(result)? {
            MintRefusal::Invalid { reason } => {
                assert_eq!(reason.as_str(), "mint.taint_required");
                Ok(())
            }
            other => Err(format!("{other:?}").into()),
        }
    };
    let issue = hyalos_types::Provenance::ForgeAuthored;
    // An actor reads its repository, so whatever it is given its manifest must be tainted.
    let mut clean = manifest(job);
    clean.tainted = false;
    required(e.mint_manifest(&clean, &StepInputs::default(), &manifest_key(), at(20)))?;
    required(e.mint_manifest(&clean, &StepInputs::of([issue]), &manifest_key(), at(20)))?;
    // A reader or planner given issue text, or model output derived from it, must be tainted too.
    for role in [Role::Reader, Role::Planner, Role::Deterministic] {
        let mut m = manifest(job);
        m.role = role;
        m.tainted = false;
        for read in [issue, hyalos_types::Provenance::ModelFromTainted] {
            required(e.mint_manifest(&m, &StepInputs::of([read]), &manifest_key(), at(20)))?;
        }
        // Given only the operator's values, it may run clean.
        let operator = StepInputs::of([hyalos_types::Provenance::Operator]);
        assert!(
            e.mint_manifest(&m, &operator, &manifest_key(), at(20))
                .is_ok(),
            "{role:?}"
        );
        // And tainted, it is minted whatever it reads: it can only do less.
        m.tainted = true;
        assert!(
            e.mint_manifest(&m, &operator, &manifest_key(), at(20))
                .is_ok(),
            "{role:?}"
        );
    }
    assert!(
        e.mint_manifest(
            &manifest(job),
            &StepInputs::of([issue]),
            &manifest_key(),
            at(20)
        )
        .is_ok()
    );
    Ok(())
}

/// S3.14: under an operator policy that permits everything, a tainted manifest still cannot merge or
/// use a subscription token (the baseline, ADR-0033). Pushing outside the branch prefix is refused for
/// every manifest by the git proxy (ADR-0046, `core/broker/tests/git.rs`, whose job is tainted).
#[test]
fn a_tainted_manifest_cannot_merge_even_when_the_operator_permits_everything() -> Res {
    let (mut e, job, _) = running("tainted-limits")?;
    e.approve_policy(
        at(10),
        "permit (principal, action, resource);",
        Decider::Human,
        why(),
    )?;
    let mut m = manifest(job);
    m.actions = set(&["merge", "read"]);
    m.approval = Some(hyalos_types::Approval {
        head_sha: "0123456789abcdef0123456789abcdef01234567".into(),
        event: 5,
    });
    match refusal(e.mint_manifest(&m, &StepInputs::default(), &manifest_key(), at(20)))? {
        MintRefusal::Denied { ask, .. } => assert_eq!(ask, "merge"),
        other => return Err(format!("{other:?}").into()),
    }
    // Every policy question minting asks of a tainted job says so, so the baseline's taint rules see it.
    let policies = e.policies()?;
    let mut request = hyalos_policy::PolicyRequest {
        action: hyalos_policy::Action::Read,
        job: hyalos_policy::JobFacts {
            tainted: true,
            repo: m.repo.clone(),
            budget_tokens: m.budget.tokens,
        },
        target: hyalos_policy::TargetFacts {
            repo: m.repo.clone(),
            name: String::new(),
            in_branch_prefix: true,
            protected: false,
        },
        approved: true,
        subscription_token: true,
    };
    assert_eq!(
        policies.judge_request(&request).answer,
        hyalos_types::Answer::Deny
    );
    request.job.tainted = false;
    assert_eq!(
        policies.judge_request(&request).answer,
        hyalos_types::Answer::Allow
    );
    Ok(())
}
