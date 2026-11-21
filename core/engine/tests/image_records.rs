//! The records the state shares with the snapshot's image are written exactly as pinned (ADR-0125).
//!
//! The state holds the image's own record types for its plain records: the policy, handles, merges,
//! plugins, install policy, pending updates, trusted root, revocation list, revoked plugins, drafts
//! and pruned archives. A field added to one of them changes the snapshot's bytes with nothing in the
//! engine to stop it, so this test builds a state with at least one of each, by replaying events, and
//! compares its image with bytes pinned from the build before the state held those types (when each
//! record was still converted field by field). If the bytes change, `STATE_VERSION` is bumped and a
//! step added (ADR-0026); this row is then replaced, never edited to match.

use hyalos_engine::State;
use hyalos_types::snapshot::StateImage;
use hyalos_types::{
    CommitSha, Digest, Event, Grant, IndexLocation, InstallPolicy, JobId, JobOutcome, MergeMethod,
    Origin, PluginName, PluginSource, PluginVersion, ReasonCode, Resolution, Signed, SubstrateName,
    TaskId, Timestamp,
};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn why() -> ReasonCode {
    ReasonCode::unavailable()
}

fn name(text: &str) -> Res<PluginName> {
    PluginName::new(text).ok_or_else(|| "a plugin name".into())
}

fn index() -> Res<IndexLocation> {
    IndexLocation::new("https://plugins.example/v1").ok_or_else(|| "an index".into())
}

fn version(text: &str) -> Res<PluginVersion> {
    PluginVersion::new(text).ok_or_else(|| "a version".into())
}

/// One of every event that leaves a shared record in the state, after a task whose job succeeded.
fn events() -> Res<Vec<Event>> {
    let task = TaskId::from_seq(1);
    Ok(vec![
        Event::TaskCreated { request: d(1) },
        Event::JobStarted {
            task,
            substrate: SubstrateName::new("test").unwrap_or_else(SubstrateName::unrecorded),
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: hyalos_types::Attestation::no_hardware_root(),
        },
        Event::JobEnded {
            job: JobId::from_seq(2),
            outcome: JobOutcome::Succeeded,
            reason: why(),
        },
        // A second task, for the merge: a task's merge is approved before any job runs on it.
        Event::TaskCreated { request: d(2) },
        Event::PolicyApproved {
            document: "permit (principal, action, resource);".into(),
            by: Resolution::Human,
            reason: why(),
            signed: Signed::Unrecorded,
        },
        Event::HandleReleased {
            task,
            handle: d(9),
            by: Resolution::Human,
            reason: why(),
            signed: Signed::Unrecorded,
            writers: None,
        },
        Event::MergeApproved {
            task: TaskId::from_seq(4),
            repo: "octo/repo".into(),
            pr: 42,
            head: CommitSha::new("0123456789abcdef0123456789abcdef01234567").ok_or("a commit")?,
            method: MergeMethod::Squash,
            by: Resolution::Human,
            reason: why(),
            signed: Signed::Unrecorded,
        },
        Event::PluginPolicySet {
            policy: InstallPolicy {
                ceiling: Grant {
                    egress_hosts: ["docs.rs".to_string()].into(),
                    ..Grant::none()
                },
                indexes: [index()?].into(),
                max_snapshot_age_days: 90,
                max_revocation_age_days: 30,
            },
            by: Resolution::Human,
            reason: why(),
            signed: Signed::Unrecorded,
        },
        Event::PluginInstalled {
            name: name("lint")?,
            digest: d(13),
            grant: Grant::none(),
            source: PluginSource::Index {
                location: index()?,
                version: version("1.2.0")?,
                signed_at: None,
            },
            by: Resolution::Human,
            reason: why(),
            signed: Signed::Unrecorded,
        },
        Event::PluginInstalled {
            name: name("pdf-tools")?,
            digest: d(12),
            grant: Grant {
                actions: ["read".to_string()].into(),
                ..Grant::none()
            },
            source: PluginSource::Local,
            by: Resolution::Human,
            reason: why(),
            signed: Signed::Unrecorded,
        },
        Event::PluginUpdatePending {
            name: name("lint")?,
            version: version("2.0.0")?,
            digest: d(14),
            asks: Grant {
                actions: ["push".to_string()].into(),
                ..Grant::none()
            },
            location: index()?,
        },
        Event::PluginSnapshotImported {
            digest: d(15),
            signed_at: Timestamp::from_millis(1_794_878_463_000),
        },
        Event::PluginRevocationsImported {
            digest: d(16),
            signed_at: Timestamp::from_millis(1_794_882_145_000),
        },
        Event::PluginRevoked {
            name: name("pdf-tools")?,
            digest: d(12),
            reason: ReasonCode::new("malware.exfiltration").ok_or("a reason")?,
        },
        Event::TaskCompleted { task },
        Event::SkillDrafted {
            name: name("draft-1")?,
            digest: d(17),
            task,
            grant: Grant::read_only(),
        },
        Event::SkillTrusted {
            name: name("draft-1")?,
            digest: d(17),
        },
        Event::ArchivePruned {
            last_seq: 3,
            last_hash: d(5),
        },
    ])
}

fn replayed() -> Res<State> {
    let mut state = State::default();
    for (n, event) in events()?.iter().enumerate() {
        let seq = u64::try_from(n)? + 1;
        // Written below the signed schema version, so an unrecorded signature is accepted.
        state.apply_written(
            seq,
            Timestamp::from_millis(1_000 + seq * 100),
            Origin::Engine,
            12,
            event,
        )?;
    }
    Ok(state)
}

#[test]
fn every_shared_record_is_written_exactly_as_pinned() -> Res {
    let state = replayed()?;
    let image = state
        .to_image()
        .map_err(|job| format!("job {job} pending"))?;
    let empty = [
        image.pruned.is_empty(),
        image.policy.is_none(),
        image.handles.is_empty(),
        image.merges.is_empty(),
        image.plugins.is_empty(),
        image.install_policy.is_none(),
        image.pending_updates.is_empty(),
        image.trusted_root.is_none(),
        image.revocations.is_none(),
        image.revoked_plugins.is_empty(),
        image.drafts.is_empty(),
    ];
    assert_eq!(empty, [false; 11], "every shared record must be populated");
    let bytes = String::from_utf8(image.encode()?)?;
    assert_eq!(bytes, PINNED);
    let back = State::from_image(StateImage::decode(
        hyalos_types::snapshot::STATE_VERSION,
        bytes.as_bytes(),
    )?)?;
    assert_eq!(back, state);
    Ok(())
}

const PINNED: &str = r#"{"last_seq":18,"last_at":2800,"tasks":[{"id":1,"created_at":1100,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":2500,"open_holds":[],"cancelled":null},{"id":4,"created_at":1400,"request":"0202020202020202020202020202020202020202020202020202020202020202","completed_at":null,"open_holds":[],"cancelled":null}],"jobs":[{"id":2,"task":1,"started_at":1200,"ended":{"at":1300,"outcome":"succeeded","reason":"gate.unavailable"},"values":null}],"holds":[],"pruned":[{"seq":18,"at":2800,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"}],"policy":{"seq":5,"at":1500,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":6,"at":1600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[],"last_approval":null,"signed_since":null,"merges":[{"seq":7,"at":1700,"task":4,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":9,"at":1900,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"},{"seq":10,"at":2000,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":[]},"source":"local","by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":8,"at":1800,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"},"pending_updates":[{"seq":11,"at":2100,"name":"lint","version":"2.0.0","digest":"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e","asks":{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]},"location":"https://plugins.example/v1"}],"trusted_root":{"seq":12,"at":2200,"digest":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","signed_at":1794878463000},"revocations":{"seq":13,"at":2300,"digest":"1010101010101010101010101010101010101010101010101010101010101010","signed_at":1794882145000},"revoked_plugins":[{"seq":14,"at":2400,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","reason":"malware.exfiltration"}],"drafts":[{"seq":16,"at":2600,"name":"draft-1","digest":"1111111111111111111111111111111111111111111111111111111111111111","task":1,"grant":{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":["job->model"]},"trusted":17}]}"#;
