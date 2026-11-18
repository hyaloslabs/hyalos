//! S6.2: a plugin is installed by a human, signed by the operator, with the files bound by their
//! digest and the grant checked against the vocabulary (ADR-0006, ADR-0012, ADR-0085, ADR-0089).

use std::path::PathBuf;

use hyalos_engine::{Decider, Engine, EngineError, RuleViolation, approvals};
use hyalos_log::{Head, LogKey};
use hyalos_types::{
    Digest, Event, Grant, IndexLocation, InstallPolicy, OperatorKey, Origin, PluginName,
    PluginSource, PluginVersion, ReasonCode, Resolution, Signed, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key() -> LogKey {
    LogKey::from_seed([9; 32])
}

fn operator() -> OperatorKey {
    OperatorKey::from_seed(&[0x0e; 32])
}

fn at(n: u64) -> Timestamp {
    Timestamp::from_millis(1_000 + n)
}

fn why() -> ReasonCode {
    ReasonCode::new("plugin.local_install").unwrap_or_else(ReasonCode::unavailable)
}

fn name(text: &str) -> Res<PluginName> {
    Ok(PluginName::new(text).ok_or("name")?)
}

fn path(test: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("plugins-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("events.db"))
}

fn docs() -> Grant {
    Grant {
        egress_hosts: ["docs.rs".to_string()].into(),
        data_flows: ["job->web".to_string()].into(),
        ..Grant::none()
    }
}

fn install(
    e: &mut Engine,
    n: u64,
    plugin: &str,
    digest: u8,
    grant: Grant,
    by: Decider,
) -> Res<Head> {
    Ok(e.install_plugin(
        at(n),
        name(plugin)?,
        Digest::from_bytes([digest; 32]),
        grant,
        PluginSource::Local,
        by,
        why(),
    )?)
}

#[test]
fn a_human_installs_a_plugin_and_a_later_install_replaces_it() -> Res {
    let mut e = Engine::open(&path("install")?, key())?.with_operator(operator());
    install(&mut e, 1, "pdf", 1, docs(), Decider::Human)?;
    install(&mut e, 2, "lint", 2, Grant::none(), Decider::Human)?;
    let pdf = e.state().plugins().get(&name("pdf")?).ok_or("pdf")?;
    assert_eq!((pdf.seq, pdf.digest), (1, Digest::from_bytes([1; 32])));
    assert_eq!(pdf.grant, docs());
    assert_eq!(
        (pdf.source.clone(), pdf.by),
        (PluginSource::Local, Resolution::Human)
    );
    // An update replaces the earlier install: one name, one set of files, one grant.
    install(&mut e, 3, "pdf", 3, Grant::none(), Decider::Human)?;
    let pdf = e.state().plugins().get(&name("pdf")?).ok_or("pdf")?;
    assert_eq!((pdf.seq, pdf.digest), (3, Digest::from_bytes([3; 32])));
    assert!(pdf.grant.is_empty());
    assert_eq!(e.state().plugins().len(), 2);
    Ok(())
}

#[test]
fn only_a_human_installs_and_only_a_grant_the_vocabulary_accepts() -> Res {
    let mut e = Engine::open(&path("rules")?, key())?.with_operator(operator());
    // Installing grants capabilities: no policy, verifier or re-evaluation may do it (ADR-0012), even
    // with an empty grant, since the plugin's text then steers a job.
    for by in [Decider::Policy, Decider::Verifier] {
        assert!(matches!(
            install(&mut e, 1, "pdf", 1, Grant::none(), by)
                .err()
                .and_then(|e| e.downcast::<EngineError>().ok())
                .map(|e| *e),
            Some(EngineError::Rule(RuleViolation::PluginNeedsAHuman { .. }))
        ));
    }
    // A grant with a name the vocabulary refuses is refused, though serde would have read it.
    let bad = Grant {
        egress_hosts: ["*.example".to_string()].into(),
        ..Grant::none()
    };
    assert!(matches!(
        install(&mut e, 2, "pdf", 1, bad, Decider::Human)
            .err()
            .and_then(|e| e.downcast::<EngineError>().ok())
            .map(|e| *e),
        Some(EngineError::Rule(RuleViolation::PluginGrantInvalid))
    ));
    assert!(e.state().plugins().is_empty());
    // Raw, it is refused whoever writes it, signed or not.
    let raw = Event::PluginInstalled {
        name: name("pdf")?,
        digest: Digest::from_bytes([1; 32]),
        grant: docs(),
        source: PluginSource::Local,
        by: Resolution::Human,
        reason: why(),
        signed: Signed::Unrecorded,
    };
    assert!(matches!(
        e.submit(at(3), Origin::Engine, raw.clone()),
        Err(EngineError::Rule(RuleViolation::ThroughTheEngineApi { .. }))
    ));
    assert!(matches!(
        e.submit(at(3), Origin::Broker, raw),
        Err(EngineError::Rule(RuleViolation::OriginNotAllowed { .. }))
    ));
    // Without the operator's key a human's install is not recorded at all.
    let mut unsigned = Engine::open(&path("unsigned")?, key())?;
    assert!(matches!(
        install(&mut unsigned, 1, "pdf", 1, docs(), Decider::Human)
            .err()
            .and_then(|e| e.downcast::<EngineError>().ok())
            .map(|e| *e),
        Some(EngineError::NoOperatorKey)
    ));
    assert!(unsigned.state().plugins().is_empty());
    Ok(())
}

#[test]
fn an_install_is_signed_survives_compaction_and_verifies() -> Res {
    let p = path("compact")?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    install(&mut e, 1, "pdf", 1, docs(), Decider::Human)?;
    let checked = approvals(&p, &[], &key().public(), Some(&operator().public()))?;
    assert_eq!(checked.signed, vec![1]);
    let archive = p.with_file_name("archive.db");
    e.compact(at(2), &archive)?;
    drop(e);
    let e = Engine::open(&p, key())?.with_operator(operator());
    let kept = e.state().plugins().get(&name("pdf")?).ok_or("lost")?;
    assert_eq!((kept.seq, kept.grant.clone()), (1, docs()));
    Ok(())
}

const TRUSTED: &str = "https://plugins.example/v1";

fn from(location: &str) -> Res<PluginSource> {
    Ok(PluginSource::Index {
        location: IndexLocation::new(location).ok_or("location")?,
        version: PluginVersion::new("1.0.0").ok_or("version")?,
    })
}

fn policy() -> Res<InstallPolicy> {
    Ok(InstallPolicy {
        ceiling: docs(),
        indexes: [IndexLocation::new(TRUSTED).ok_or("location")?].into(),
        max_snapshot_age_days: 90,
        max_revocation_age_days: 30,
    })
}

fn install_from(
    e: &mut Engine,
    n: u64,
    source: PluginSource,
    grant: Grant,
    by: Decider,
) -> Result<Head, EngineError> {
    e.install_plugin(
        at(n),
        PluginName::new("pdf").unwrap_or_else(|| unreachable!()),
        Digest::from_bytes([1; 32]),
        grant,
        source,
        by,
        why(),
    )
}

#[test]
fn only_a_human_sets_the_install_policy_and_only_a_valid_one() -> Res {
    let mut e = Engine::open(&path("policy-set")?, key())?.with_operator(operator());
    for by in [Decider::Policy, Decider::Verifier] {
        assert!(matches!(
            e.set_install_policy(at(1), policy()?, by, why()),
            Err(EngineError::Rule(
                RuleViolation::InstallPolicyNeedsAHuman { .. }
            ))
        ));
    }
    let mut bad = policy()?;
    bad.max_revocation_age_days = 0;
    assert!(matches!(
        e.set_install_policy(at(1), bad, Decider::Human, why()),
        Err(EngineError::Rule(RuleViolation::InstallPolicyInvalid))
    ));
    let raw = Event::PluginPolicySet {
        policy: policy()?,
        by: Resolution::Human,
        reason: why(),
        signed: Signed::Unrecorded,
    };
    assert!(matches!(
        e.submit(at(2), Origin::Engine, raw),
        Err(EngineError::Rule(RuleViolation::ThroughTheEngineApi { .. }))
    ));
    assert!(e.state().install_policy().is_none());
    e.set_install_policy(at(3), policy()?, Decider::Human, why())?;
    let set = e.state().install_policy().ok_or("no policy")?;
    assert_eq!((set.seq, &set.policy), (1, &policy()?));
    Ok(())
}

#[test]
fn policy_installs_only_from_a_trusted_index_within_the_ceiling_and_never_a_verifier() -> Res {
    let p = path("policy-install")?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    let refused = |r: Result<Head, EngineError>| {
        matches!(
            r,
            Err(EngineError::Rule(RuleViolation::PluginNeedsAHuman { .. }))
        )
    };
    // No policy yet: policy installs nothing.
    assert!(refused(install_from(
        &mut e,
        1,
        from(TRUSTED)?,
        Grant::none(),
        Decider::Policy
    )));
    e.set_install_policy(at(2), policy()?, Decider::Human, why())?;
    // Within the ceiling, from the trusted index: installed, by policy, unsigned.
    let within = Grant {
        egress_hosts: ["docs.rs".to_string()].into(),
        ..Grant::none()
    };
    install_from(&mut e, 3, from(TRUSTED)?, within.clone(), Decider::Policy)?;
    let pdf = e.state().plugins().get(&name("pdf")?).ok_or("pdf")?;
    assert_eq!(pdf.by, Resolution::Policy);
    // Beyond the ceiling, from another index, or local: a human's decision.
    let beyond = Grant {
        actions: ["push".to_string()].into(),
        ..Grant::none()
    };
    assert!(refused(install_from(
        &mut e,
        4,
        from(TRUSTED)?,
        beyond.clone(),
        Decider::Policy
    )));
    assert!(refused(install_from(
        &mut e,
        5,
        from("https://other.example/v1")?,
        within.clone(),
        Decider::Policy
    )));
    assert!(refused(install_from(
        &mut e,
        6,
        PluginSource::Local,
        Grant::none(),
        Decider::Policy
    )));
    // A verifier never grants, even within the policy (ADR-0012).
    assert!(refused(install_from(
        &mut e,
        7,
        from(TRUSTED)?,
        within,
        Decider::Verifier
    )));
    // The human still may.
    install_from(&mut e, 8, from(TRUSTED)?, beyond, Decider::Human)?;
    // Only the human decisions are signed, and the policy survives compaction.
    let checked = approvals(&p, &[], &key().public(), Some(&operator().public()))?;
    assert_eq!(checked.signed, vec![1, 3]);
    e.compact(at(9), &p.with_file_name("archive.db"))?;
    drop(e);
    let e = Engine::open(&p, key())?.with_operator(operator());
    assert_eq!(
        e.state().install_policy().map(|s| s.policy.clone()),
        Some(policy()?)
    );
    Ok(())
}

#[test]
fn an_update_waits_for_an_index_install_until_any_install_answers_it() -> Res {
    let mut e = Engine::open(&path("pending")?, key())?.with_operator(operator());
    let pending = |version: &str, digest: u8| -> Res<Event> {
        Ok(Event::PluginUpdatePending {
            name: name("pdf")?,
            version: PluginVersion::new(version).ok_or("version")?,
            digest: Digest::from_bytes([digest; 32]),
            asks: Grant {
                actions: ["push".to_string()].into(),
                ..Grant::none()
            },
            location: IndexLocation::new(TRUSTED).ok_or("location")?,
        })
    };
    // Not installed, or installed locally: nothing can wait for it.
    assert!(matches!(
        e.submit(at(1), Origin::Engine, pending("2", 2)?),
        Err(EngineError::Rule(RuleViolation::UpdateNotForAnIndexInstall))
    ));
    install_from(
        &mut e,
        2,
        PluginSource::Local,
        Grant::none(),
        Decider::Human,
    )?;
    assert!(matches!(
        e.submit(at(3), Origin::Engine, pending("2", 2)?),
        Err(EngineError::Rule(RuleViolation::UpdateNotForAnIndexInstall))
    ));
    install_from(&mut e, 4, from(TRUSTED)?, Grant::none(), Decider::Human)?;
    // The broker may not say one is waiting.
    assert!(e.submit(at(5), Origin::Broker, pending("2", 2)?).is_err());
    e.submit(at(6), Origin::Engine, pending("2", 2)?)?;
    let waiting = e
        .state()
        .pending_updates()
        .get(&name("pdf")?)
        .ok_or("waiting")?;
    assert_eq!(waiting.version.as_str(), "2");
    // The installed version is still the one in force.
    assert_eq!(e.state().plugins().get(&name("pdf")?).ok_or("pdf")?.seq, 2);
    // A later find replaces it; an install of the name answers it.
    e.submit(at(7), Origin::Engine, pending("3", 3)?)?;
    assert_eq!(e.state().pending_updates().len(), 1);
    install_from(&mut e, 8, from(TRUSTED)?, Grant::none(), Decider::Human)?;
    assert!(e.state().pending_updates().is_empty());
    Ok(())
}

#[test]
fn a_trusted_root_snapshot_is_imported_only_newer_and_survives_compaction() -> Res {
    let p = path("snapshot")?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    let digest = |b: u8| Digest::from_bytes([b; 32]);
    assert_eq!(e.state().trusted_root(), None);
    e.import_snapshot(at(1), digest(1), Timestamp::from_millis(5_000))?;
    // The same issue time, or an earlier one, is a rollback, and the log never records one.
    for older in [5_000, 4_999] {
        assert!(matches!(
            e.import_snapshot(at(2), digest(2), Timestamp::from_millis(older)),
            Err(EngineError::Rule(RuleViolation::SnapshotNotNewer))
        ));
    }
    e.import_snapshot(at(3), digest(3), Timestamp::from_millis(6_000))?;
    let last = e.state().trusted_root().copied().ok_or("imported")?;
    assert_eq!(
        (last.digest, last.signed_at),
        (digest(3), Timestamp::from_millis(6_000))
    );
    e.compact(at(4), &p.with_file_name("archive.db"))?;
    drop(e);
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    assert_eq!(e.state().trusted_root().copied(), Some(last));
    assert!(matches!(
        e.import_snapshot(at(5), digest(4), Timestamp::from_millis(6_000)),
        Err(EngineError::Rule(RuleViolation::SnapshotNotNewer))
    ));
    Ok(())
}

#[test]
fn a_revoked_release_is_recorded_for_the_one_installed_and_never_installed_again() -> Res {
    let p = path("revoked")?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    let digest = |b: u8| Digest::from_bytes([b; 32]);
    let malware = || ReasonCode::new("malware.exfiltration").unwrap_or_else(|| unreachable!());
    install(&mut e, 1, "pdf", 1, Grant::none(), Decider::Human)?;
    // Nothing is revoked before a list is imported.
    assert!(matches!(
        e.revoke_plugin(at(2), name("pdf")?, digest(1), malware()),
        Err(EngineError::Rule(RuleViolation::RevocationNotApplicable))
    ));
    e.import_revocations(at(3), digest(9), Timestamp::from_millis(5_000))?;
    for older in [5_000, 4_999] {
        assert!(matches!(
            e.import_revocations(at(4), digest(8), Timestamp::from_millis(older)),
            Err(EngineError::Rule(RuleViolation::RevocationsNotNewer))
        ));
    }
    // Only the release installed under the name, and a name that is installed.
    for (plugin, d) in [("pdf", 2), ("lint", 1)] {
        assert!(matches!(
            e.revoke_plugin(at(5), name(plugin)?, digest(d), malware()),
            Err(EngineError::Rule(RuleViolation::RevocationNotApplicable))
        ));
    }
    e.revoke_plugin(at(6), name("pdf")?, digest(1), malware())?;
    assert!(matches!(
        e.revoke_plugin(at(7), name("pdf")?, digest(1), malware()),
        Err(EngineError::Rule(RuleViolation::RevocationNotApplicable))
    ));
    let revoked = e.state().revoked_plugins().get(&name("pdf")?).cloned();
    assert_eq!(revoked.as_ref().map(|r| r.digest), Some(digest(1)));
    // The revoked release is never installed again; another one replaces it, and is not revoked.
    assert!(matches!(
        install(&mut e, 8, "pdf", 1, Grant::none(), Decider::Human)
            .map_err(|e| e.downcast::<EngineError>().map(|e| *e)),
        Err(Ok(EngineError::Rule(RuleViolation::PluginRevoked)))
    ));
    e.compact(at(9), &p.with_file_name("archive.db"))?;
    drop(e);
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    assert_eq!(
        e.state().revoked_plugins().get(&name("pdf")?).cloned(),
        revoked
    );
    assert_eq!(
        e.state().revocations().map(|r| r.signed_at),
        Some(Timestamp::from_millis(5_000))
    );
    install(&mut e, 10, "pdf", 2, Grant::none(), Decider::Human)?;
    assert!(e.state().revoked_plugins().is_empty());
    assert!(
        e.state().invariant_violations().is_empty(),
        "{:?}",
        e.state().invariant_violations()
    );
    Ok(())
}

/// S9.2: a skill is drafted only from a task whose last job succeeded, once per name, held to read
/// capabilities, and the state keeps it through compaction (ADR-0113).
#[test]
fn a_draft_comes_only_from_a_success_holds_only_read_and_survives_compaction() -> Res {
    use hyalos_types::{Attestation, CredentialTier, JobId, JobOutcome, SubstrateName, TaskId};
    let p = path("drafts")?;
    let mut e = Engine::open(&p, key())?.with_operator(operator());
    let mut n = 0;
    let mut submit = |e: &mut Engine, event: Event| {
        n += 1;
        e.submit(at(n), Origin::Engine, event)
    };
    // Task 1 runs one job, which ends `outcome`; the task completes unless it is `None`.
    let mut run = |e: &mut Engine, outcome: Option<JobOutcome>| -> Res<TaskId> {
        let task = TaskId::from_seq(
            submit(
                e,
                Event::TaskCreated {
                    request: Digest::from_bytes([1; 32]),
                },
            )?
            .seq,
        );
        let job = JobId::from_seq(
            submit(
                e,
                Event::JobStarted {
                    task,
                    substrate: SubstrateName::new("broker").ok_or("substrate")?,
                    credentials: CredentialTier::BrokerHeld,
                    attestation: Attestation::no_hardware_root(),
                },
            )?
            .seq,
        );
        if let Some(outcome) = outcome {
            submit(
                e,
                Event::JobEnded {
                    job,
                    outcome,
                    reason: why(),
                },
            )?;
            submit(e, Event::TaskCompleted { task })?;
        }
        Ok(task)
    };
    let open = run(&mut e, None)?;
    let failed = run(&mut e, Some(JobOutcome::Failed))?;
    let succeeded = run(&mut e, Some(JobOutcome::Succeeded))?;
    install(&mut e, 90, "lint", 2, Grant::none(), Decider::Human)?;
    let draft = |plugin: &str, task, grant| -> Res<Event> {
        Ok(Event::SkillDrafted {
            name: name(plugin)?,
            digest: Digest::from_bytes([7; 32]),
            task,
            grant,
        })
    };
    let wider = Grant {
        actions: ["read".to_string(), "push".to_string()].into(),
        ..Grant::read_only()
    };
    for refused in [
        draft("draft-a", open, Grant::read_only())?,
        draft("draft-b", failed, Grant::read_only())?,
        draft("draft-c", succeeded, wider)?,
        draft("lint", succeeded, Grant::read_only())?,
    ] {
        assert!(
            matches!(
                e.submit(at(100), Origin::Engine, refused.clone()),
                Err(EngineError::Rule(RuleViolation::DraftRefused))
            ),
            "{refused:?}"
        );
    }
    // The broker never drafts.
    let ok = draft("draft-ok", succeeded, Grant::read_only())?;
    assert!(e.submit(at(100), Origin::Broker, ok.clone()).is_err());
    let drafted = e.submit(at(100), Origin::Engine, ok.clone())?;
    assert!(matches!(
        e.submit(at(101), Origin::Engine, ok),
        Err(EngineError::Rule(RuleViolation::DraftRefused))
    ));
    let kept = e
        .state()
        .drafts()
        .get(&name("draft-ok")?)
        .cloned()
        .ok_or("drafted")?;
    assert_eq!(
        (kept.seq, kept.task, kept.grant.clone()),
        (drafted.seq, succeeded, Grant::read_only())
    );
    e.compact(at(102), &p.with_file_name("archive.db"))?;
    drop(e);
    let e = Engine::open(&p, key())?;
    assert_eq!(e.state().drafts().get(&name("draft-ok")?), Some(&kept));
    Ok(())
}
