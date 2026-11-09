//! S6.2: a plugin is installed by a human, signed by the operator, with the files bound by their
//! digest and the grant checked against the vocabulary (ADR-0006, ADR-0012, ADR-0085, ADR-0089).

use std::path::PathBuf;

use hyalos_engine::{Decider, Engine, EngineError, RuleViolation, approvals};
use hyalos_log::{Head, LogKey};
use hyalos_types::{
    Digest, Event, Grant, OperatorKey, Origin, PluginName, PluginSource, ReasonCode, Resolution,
    Signed, Timestamp,
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
