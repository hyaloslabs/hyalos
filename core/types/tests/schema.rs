//! Event schema versions and upcasting (ADR-0023, S1.4).

use hyalos_types::event::{SCHEMA, SCHEMA_VERSION};
use hyalos_types::{
    Answer, Attestation, Digest, Event, EventError, Gate, HoldId, HoldReason, JobId, JobOutcome,
    ReasonCode, Resolution, Schema, Sensitive, Signed, Stakes, SubstrateName, TaskId, Upcast,
};
use serde_json::{Value, json};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

// ---- the pinned payloads: every kind, as each released version wrote it -------------------------

/// `(version, kind, payload as that version wrote it, the event it means)`. Add the new version's
/// rows when `SCHEMA_VERSION` is bumped; never edit an existing row, since logs hold these bytes.
fn golden() -> Res<Vec<(u32, &'static str, String, Event)>> {
    let version_1 = vec![
        (
            1,
            "task.created",
            format!(r#"{{"kind":"task.created","request":"{}"}}"#, hex(1)),
            Event::TaskCreated { request: d(1) },
        ),
        (
            1,
            "task.completed",
            r#"{"kind":"task.completed","task":3}"#.into(),
            Event::TaskCompleted {
                task: TaskId::from_seq(3),
            },
        ),
        (
            1,
            "job.started",
            r#"{"kind":"job.started","task":3}"#.into(),
            // Before version 6 a job did not name its substrate; it reads as unrecorded.
            Event::JobStarted {
                task: TaskId::from_seq(3),
                substrate: SubstrateName::unrecorded(),
                credentials: hyalos_types::CredentialTier::Unrecorded,
                attestation: Attestation::Unrecorded,
            },
        ),
        (
            1,
            "job.ended",
            r#"{"kind":"job.ended","job":4,"outcome":"deadline_exceeded"}"#.into(),
            // Before version 8 a job's end did not say why; it reads as unrecorded.
            Event::JobEnded {
                job: JobId::from_seq(4),
                outcome: JobOutcome::DeadlineExceeded,
                reason: ReasonCode::new("legacy.unrecorded").ok_or("reason")?,
            },
        ),
        (
            1,
            "gate.answered",
            format!(
                r#"{{"kind":"gate.answered","job":4,"gate":"content","inputs":"{}","answer":"deny","reason":"content.secret_pattern"}}"#,
                hex(6)
            ),
            Event::GateAnswered {
                job: JobId::from_seq(4),
                gate: Gate::Content,
                inputs: d(6),
                answer: Answer::Deny,
                reason: ReasonCode::new("content.secret_pattern").ok_or("reason")?,
            },
        ),
        (
            1,
            "hold.opened",
            format!(
                r#"{{"kind":"hold.opened","task":3,"gate":"egress","inputs":"{}","reason":"timed_out"}}"#,
                hex(2)
            ),
            // A version 1 hold had no stakes; it reads as the most cautious (a human must release it).
            Event::HoldOpened {
                task: TaskId::from_seq(3),
                gate: Gate::Egress,
                inputs: d(2),
                reason: HoldReason::TimedOut,
                stakes: Stakes::irreversible(),
            },
        ),
        (
            1,
            "hold.resolved",
            r#"{"kind":"hold.resolved","hold":5,"by":"human"}"#.into(),
            // A version 1 resolution had no reason; it reads as explicitly unrecorded.
            Event::HoldResolved {
                hold: HoldId::from_seq(5),
                by: Resolution::Human,
                reason: ReasonCode::new("legacy.unrecorded").ok_or("reason")?,
                signed: Signed::Unrecorded,
            },
        ),
    ];

    // Version 2 wrote five kinds exactly as version 1 did, and the two below differently.
    let mut all = version_1.clone();
    for (_, kind, json, event) in &version_1 {
        if !kind.starts_with("hold.") {
            all.push((2, kind, json.clone(), event.clone()));
        }
    }
    all.push((
        2,
        "hold.opened",
        format!(
            r#"{{"kind":"hold.opened","task":3,"gate":"egress","inputs":"{}","reason":"timed_out","stakes":{{"tier":"escalated","touches":["credentials","capability_grant"]}}}}"#,
            hex(2)
        ),
        Event::HoldOpened {
            task: TaskId::from_seq(3),
            gate: Gate::Egress,
            inputs: d(2),
            reason: HoldReason::TimedOut,
            stakes: Stakes::escalated()
                .touching(Sensitive::Credentials)
                .touching(Sensitive::CapabilityGrant),
        },
    ));
    all.push((
        2,
        "hold.resolved",
        r#"{"kind":"hold.resolved","hold":5,"by":"human","reason":"approved.operator"}"#.into(),
        Event::HoldResolved {
            hold: HoldId::from_seq(5),
            by: Resolution::Human,
            reason: ReasonCode::new("approved.operator").ok_or("reason")?,
            signed: Signed::Unrecorded,
        },
    ));

    // Version 3 added `archive.pruned` and changed no payload, so the seven older kinds read as they
    // did at version 2.
    let at_2: Vec<_> = all.iter().filter(|g| g.0 == 2).cloned().collect();
    for (_, kind, json, event) in at_2 {
        all.push((3, kind, json, event));
    }
    all.push((
        3,
        "archive.pruned",
        format!(
            r#"{{"kind":"archive.pruned","last_seq":42,"last_hash":"{}"}}"#,
            hex(7)
        ),
        Event::ArchivePruned {
            last_seq: 42,
            last_hash: d(7),
        },
    ));

    // Version 4 added `task.cancelled` and `hold.refused` and changed no payload, so the eight older
    // kinds read as they did at version 3.
    let at_3: Vec<_> = all.iter().filter(|g| g.0 == 3).cloned().collect();
    for (_, kind, json, event) in at_3 {
        all.push((4, kind, json, event));
    }
    all.push((
        4,
        "task.cancelled",
        r#"{"kind":"task.cancelled","task":3,"by":"human","reason":"cancelled.operator"}"#.into(),
        Event::TaskCancelled {
            task: TaskId::from_seq(3),
            by: Resolution::Human,
            reason: ReasonCode::new("cancelled.operator").ok_or("reason")?,
            signed: Signed::Unrecorded,
        },
    ));
    all.push((
        4,
        "hold.refused",
        r#"{"kind":"hold.refused","hold":5,"reason":"egress.denied"}"#.into(),
        Event::HoldRefused {
            hold: HoldId::from_seq(5),
            reason: ReasonCode::new("egress.denied").ok_or("reason")?,
        },
    ));

    // Version 5 added `policy.approved` and changed no payload, so the ten older kinds read as they did
    // at version 4.
    let at_4: Vec<_> = all.iter().filter(|g| g.0 == 4).cloned().collect();
    for (_, kind, json, event) in at_4 {
        all.push((5, kind, json, event));
    }
    all.push((
        5,
        "policy.approved",
        r#"{"kind":"policy.approved","document":"permit (principal, action, resource);","by":"human","reason":"policy.approved"}"#.into(),
        Event::PolicyApproved {
            document: "permit (principal, action, resource);".into(),
            by: Resolution::Human,
            reason: ReasonCode::new("policy.approved").ok_or("reason")?,
            signed: Signed::Unrecorded,        },
    ));

    // Version 6 added the substrate to `job.started`, so the ten other kinds read as they did at
    // version 5.
    let at_5: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 5 && g.1 != "job.started")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_5 {
        all.push((6, kind, json, event));
    }
    all.push((
        6,
        "job.started",
        r#"{"kind":"job.started","task":3,"substrate":"container.podman"}"#.into(),
        Event::JobStarted {
            task: TaskId::from_seq(3),
            substrate: SubstrateName::new("container.podman").ok_or("substrate")?,
            credentials: hyalos_types::CredentialTier::Unrecorded,
            attestation: Attestation::Unrecorded,
        },
    ));

    // Version 7 added the credential tier to `job.started`, so the ten other kinds read as they did at
    // version 6.
    let at_6: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 6 && g.1 != "job.started")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_6 {
        all.push((7, kind, json, event));
    }
    all.push((
        7,
        "job.started",
        r#"{"kind":"job.started","task":3,"substrate":"container.docker","credentials":"broker_held"}"#
            .into(),
        Event::JobStarted {
            task: TaskId::from_seq(3),
            substrate: SubstrateName::new("container.docker").ok_or("substrate")?,
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: Attestation::Unrecorded,
        },
    ));

    // Version 8 added the reason to `job.ended`, so the ten other kinds read as they did at version 7.
    let at_7: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 7 && g.1 != "job.ended")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_7 {
        all.push((8, kind, json, event));
    }
    all.push((
        8,
        "job.ended",
        r#"{"kind":"job.ended","job":4,"outcome":"failed","reason":"run.interrupted"}"#.into(),
        Event::JobEnded {
            job: JobId::from_seq(4),
            outcome: JobOutcome::Failed,
            reason: ReasonCode::new("run.interrupted").ok_or("reason")?,
        },
    ));

    // Version 9 added the attestation to `job.started`, so the ten other kinds read as they did at
    // version 8.
    let at_8: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 8 && g.1 != "job.started")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_8 {
        all.push((9, kind, json, event));
    }
    all.push((
        9,
        "job.started",
        r#"{"kind":"job.started","task":3,"substrate":"gvisor.docker","credentials":"broker_held","attestation":{"none":"attestation.no_hardware_root"}}"#
            .into(),
        Event::JobStarted {
            task: TaskId::from_seq(3),
            substrate: SubstrateName::new("gvisor.docker").ok_or("substrate")?,
            credentials: hyalos_types::CredentialTier::BrokerHeld,
            attestation: Attestation::no_hardware_root(),
        },
    ));

    // Version 10 added `handle.released` and changed no payload, so the eleven older kinds read as they
    // did at version 9.
    let at_9: Vec<_> = all.iter().filter(|g| g.0 == 9).cloned().collect();
    for (_, kind, json, event) in at_9 {
        all.push((10, kind, json, event));
    }
    all.push((
        10,
        "handle.released",
        format!(
            r#"{{"kind":"handle.released","task":3,"handle":"{}","by":"human","reason":"handle.operator"}}"#,
            hex(9)
        ),
        Event::HandleReleased {
            task: TaskId::from_seq(3),
            handle: d(9),
            by: Resolution::Human,
            reason: ReasonCode::new("handle.operator").ok_or("reason")?,
            signed: Signed::Unrecorded,
            writers: None,
        },
    ));

    // Version 11 added `agent.issued` and `agent.revoked` and changed no payload, so the twelve older
    // kinds read as they did at version 10.
    let at_10: Vec<_> = all.iter().filter(|g| g.0 == 10).cloned().collect();
    for (_, kind, json, event) in at_10 {
        all.push((11, kind, json, event));
    }
    all.push((
        11,
        "agent.issued",
        format!(
            r#"{{"kind":"agent.issued","name":"claude-code","credential":"{}","by":"human","reason":"agent.issued"}}"#,
            hex(5)
        ),
        Event::AgentIssued {
            name: hyalos_types::AgentName::new("claude-code").ok_or("name")?,
            credential: d(5),
            by: Resolution::Human,
            reason: ReasonCode::new("agent.issued").ok_or("reason")?,
            signed: Signed::Unrecorded,        },
    ));
    all.push((
        11,
        "agent.revoked",
        r#"{"kind":"agent.revoked","agent":4,"by":"human","reason":"agent.revoked"}"#.into(),
        Event::AgentRevoked {
            agent: hyalos_types::AgentId::from_seq(4),
            by: Resolution::Human,
            reason: ReasonCode::new("agent.revoked").ok_or("reason")?,
            signed: Signed::Unrecorded,
        },
    ));

    // Version 12 added `job.values` and changed no payload, so the fourteen older kinds read as they
    // did at version 11.
    let at_11: Vec<_> = all.iter().filter(|g| g.0 == 11).cloned().collect();
    for (_, kind, json, event) in at_11 {
        all.push((12, kind, json, event));
    }
    all.push((
        12,
        "job.values",
        r#"{"kind":"job.values","job":4,"values":[{"choice":2},{"bool":true},{"integer":-3}]}"#
            .into(),
        Event::JobValues {
            job: JobId::from_seq(4),
            values: vec![
                hyalos_types::TypedValue::Choice(2),
                hyalos_types::TypedValue::Bool(true),
                hyalos_types::TypedValue::Integer(-3),
            ],
        },
    ));
    // A version 12 decision by anyone but a human reads as unsigned, which is all it needs.
    all.push((
        12,
        "agent.revoked",
        r#"{"kind":"agent.revoked","agent":4,"by":"policy","reason":"agent.revoked"}"#.into(),
        Event::AgentRevoked {
            agent: hyalos_types::AgentId::from_seq(4),
            by: Resolution::Policy,
            reason: ReasonCode::new("agent.revoked").ok_or("reason")?,
            signed: Signed::None,
        },
    ));

    // Version 13 added `signed` to the six kinds that record a decision (ADR-0070); the others read as
    // they did at version 12. A human decision carries the operator's signature.
    let at_12: Vec<_> = all.iter().filter(|g| g.0 == 12).cloned().collect();
    for (_, kind, json, event) in at_12 {
        // One row a kind at the current version; `hold.resolved` is the signed one below.
        if kind == "hold.resolved" || all.iter().any(|g| g.0 == 13 && g.1 == kind) {
            continue;
        }
        let Some(signed) = event.signed() else {
            all.push((13, kind, json, event));
            continue;
        };
        let value = serde_json::to_string(signed)?;
        let json = format!("{},\"signed\":{value}}}", json.trim_end_matches('}'));
        all.push((13, kind, json, event));
    }
    let approval = hyalos_types::OperatorKey::from_seed(&[7; 32]).approve(&[1; 32], 4, d(2), b"x");
    all.push((
        13,
        "hold.resolved",
        format!(
            r#"{{"kind":"hold.resolved","hold":5,"by":"human","reason":"approved.operator","signed":{{"operator":{{"operator":"{}","after":4,"after_hash":"{}","signature":"{}"}}}}}}"#,
            approval.operator,
            hex(2),
            hyalos_types::hex::to_hex(approval.signature.as_bytes())
        ),
        Event::HoldResolved {
            hold: HoldId::from_seq(5),
            by: Resolution::Human,
            reason: ReasonCode::new("approved.operator").ok_or("reason")?,
            signed: Signed::Operator(approval),
        },
    ));

    // Version 14 added `merge.approved` (ADR-0071) and changed no payload, so the fifteen older kinds
    // read as they did at version 13.
    let at_13: Vec<_> = all.iter().filter(|g| g.0 == 13).cloned().collect();
    for (_, kind, json, event) in at_13 {
        all.push((14, kind, json, event));
    }
    let head = "0123456789abcdef0123456789abcdef01234567";
    let approval = hyalos_types::OperatorKey::from_seed(&[7; 32]).approve(&[1; 32], 2, d(3), b"y");
    all.push((
        14,
        "merge.approved",
        format!(
            r#"{{"kind":"merge.approved","task":1,"repo":"octo/repo","pr":42,"head":"{head}","method":"merge","by":"human","reason":"merge.operator","signed":{{"operator":{{"operator":"{}","after":2,"after_hash":"{}","signature":"{}"}}}}}}"#,
            approval.operator,
            hex(3),
            hyalos_types::hex::to_hex(approval.signature.as_bytes())
        ),
        Event::MergeApproved {
            task: TaskId::from_seq(1),
            repo: "octo/repo".into(),
            pr: 42,
            head: hyalos_types::CommitSha::new(head).ok_or("head")?,
            method: hyalos_types::MergeMethod::Merge,
            by: Resolution::Human,
            reason: ReasonCode::new("merge.operator").ok_or("reason")?,
            signed: Signed::Operator(approval),
        },
    ));

    // Version 15 added `plugin.installed` (ADR-0089) and changed no payload, so the sixteen older
    // kinds read as they did at version 14.
    let at_14: Vec<_> = all.iter().filter(|g| g.0 == 14).cloned().collect();
    for (_, kind, json, event) in at_14 {
        all.push((15, kind, json, event));
    }
    let approval = hyalos_types::OperatorKey::from_seed(&[7; 32]).approve(&[1; 32], 2, d(3), b"p");
    all.push((
        15,
        "plugin.installed",
        format!(
            r#"{{"kind":"plugin.installed","name":"pdf-tools","digest":"{}","grant":{{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]}},"source":"local","by":"human","reason":"plugin.local_install","signed":{{"operator":{{"operator":"{}","after":2,"after_hash":"{}","signature":"{}"}}}}}}"#,
            hex(6),
            approval.operator,
            hex(3),
            hyalos_types::hex::to_hex(approval.signature.as_bytes())
        ),
        Event::PluginInstalled {
            name: hyalos_types::PluginName::new("pdf-tools").ok_or("name")?,
            digest: d(6),
            grant: hyalos_types::Grant {
                actions: ["read".to_string()].into(),
                egress_hosts: ["docs.rs".to_string()].into(),
                data_flows: ["job->web".to_string()].into(),
                ..hyalos_types::Grant::none()
            },
            source: hyalos_types::PluginSource::Local,
            by: Resolution::Human,
            reason: ReasonCode::new("plugin.local_install").ok_or("reason")?,
            signed: Signed::Operator(approval),
        },
    ));

    // Version 16 added an index as a `plugin.installed` source (ADR-0090) and changed no payload, so
    // every kind written at version 15 reads as it did. Its `plugin.installed` sample is from an index.
    let at_15: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 15 && g.1 != "plugin.installed")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_15 {
        all.push((16, kind, json, event));
    }
    let approval = hyalos_types::OperatorKey::from_seed(&[7; 32]).approve(&[1; 32], 3, d(4), b"q");
    all.push((
        16,
        "plugin.installed",
        format!(
            r#"{{"kind":"plugin.installed","name":"lint","digest":"{}","grant":{{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]}},"source":{{"index":{{"location":"https://plugins.example/v1","version":"1.2.0"}}}},"by":"human","reason":"plugin.installed","signed":{{"operator":{{"operator":"{}","after":3,"after_hash":"{}","signature":"{}"}}}}}}"#,
            hex(7),
            approval.operator,
            hex(4),
            hyalos_types::hex::to_hex(approval.signature.as_bytes())
        ),
        Event::PluginInstalled {
            name: hyalos_types::PluginName::new("lint").ok_or("name")?,
            digest: d(7),
            grant: hyalos_types::Grant::none(),
            source: hyalos_types::PluginSource::Index {
                location: hyalos_types::IndexLocation::new("https://plugins.example/v1")
                    .ok_or("location")?,
                version: hyalos_types::PluginVersion::new("1.2.0").ok_or("version")?,
            },
            by: Resolution::Human,
            reason: ReasonCode::new("plugin.installed").ok_or("reason")?,
            signed: Signed::Operator(approval),
        },
    ));

    // Version 17 added `plugin.policy_set` (ADR-0091) and changed no payload.
    let at_16: Vec<_> = all.iter().filter(|g| g.0 == 16).cloned().collect();
    for (_, kind, json, event) in at_16 {
        all.push((17, kind, json, event));
    }
    let approval = hyalos_types::OperatorKey::from_seed(&[7; 32]).approve(&[1; 32], 4, d(5), b"r");
    all.push((
        17,
        "plugin.policy_set",
        format!(
            r#"{{"kind":"plugin.policy_set","policy":{{"ceiling":{{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":[]}},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30}},"by":"human","reason":"plugin.policy_set","signed":{{"operator":{{"operator":"{}","after":4,"after_hash":"{}","signature":"{}"}}}}}}"#,
            approval.operator,
            hex(5),
            hyalos_types::hex::to_hex(approval.signature.as_bytes())
        ),
        Event::PluginPolicySet {
            policy: hyalos_types::InstallPolicy {
                ceiling: hyalos_types::Grant {
                    actions: ["read".to_string()].into(),
                    ..hyalos_types::Grant::none()
                },
                indexes: [hyalos_types::IndexLocation::new("https://plugins.example/v1")
                    .ok_or("location")?]
                .into(),
                max_snapshot_age_days: 90,
                max_revocation_age_days: 30,
            },
            by: Resolution::Human,
            reason: ReasonCode::new("plugin.policy_set").ok_or("reason")?,
            signed: Signed::Operator(approval),
        },
    ));

    // Version 18 added `plugin.update_pending` (ADR-0092) and changed no payload.
    let at_17: Vec<_> = all.iter().filter(|g| g.0 == 17).cloned().collect();
    for (_, kind, json, event) in at_17 {
        all.push((18, kind, json, event));
    }
    all.push((
        18,
        "plugin.update_pending",
        format!(
            r#"{{"kind":"plugin.update_pending","name":"lint","version":"2.0.0","digest":"{}","asks":{{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]}},"location":"https://plugins.example/v1"}}"#,
            hex(8)
        ),
        Event::PluginUpdatePending {
            name: hyalos_types::PluginName::new("lint").ok_or("name")?,
            version: hyalos_types::PluginVersion::new("2.0.0").ok_or("version")?,
            digest: d(8),
            asks: hyalos_types::Grant {
                actions: ["push".to_string()].into(),
                ..hyalos_types::Grant::none()
            },
            location: hyalos_types::IndexLocation::new("https://plugins.example/v1")
                .ok_or("location")?,
        },
    ));

    // Version 19 added `plugin.snapshot_imported` (ADR-0097) and changed no payload.
    let at_18: Vec<_> = all.iter().filter(|g| g.0 == 18).cloned().collect();
    for (_, kind, json, event) in at_18 {
        all.push((19, kind, json, event));
    }
    all.push((
        19,
        "plugin.snapshot_imported",
        format!(
            r#"{{"kind":"plugin.snapshot_imported","digest":"{}","signed_at":1794878463000}}"#,
            hex(9)
        ),
        Event::PluginSnapshotImported {
            digest: d(9),
            signed_at: hyalos_types::Timestamp::from_millis(1_794_878_463_000),
        },
    ));

    // Version 20 added `plugin.revocations_imported` and `plugin.revoked` (ADR-0098) and changed no
    // payload.
    let at_19: Vec<_> = all.iter().filter(|g| g.0 == 19).cloned().collect();
    for (_, kind, json, event) in at_19 {
        all.push((20, kind, json, event));
    }
    all.push((
        20,
        "plugin.revocations_imported",
        format!(
            r#"{{"kind":"plugin.revocations_imported","digest":"{}","signed_at":1794882145000}}"#,
            hex(10)
        ),
        Event::PluginRevocationsImported {
            digest: d(10),
            signed_at: hyalos_types::Timestamp::from_millis(1_794_882_145_000),
        },
    ));
    all.push((
        20,
        "plugin.revoked",
        format!(
            r#"{{"kind":"plugin.revoked","name":"lint","digest":"{}","reason":"malware.exfiltration"}}"#,
            hex(11)
        ),
        Event::PluginRevoked {
            name: hyalos_types::PluginName::new("lint").ok_or("name")?,
            digest: d(11),
            reason: ReasonCode::new("malware.exfiltration").ok_or("reason")?,
        },
    ));

    // Version 21 added `writers` to `handle.released` (ADR-0103), so every other kind reads as it did
    // at version 20, and a release written before has none. Its sample is a release by policy.
    let at_20: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 20 && g.1 != "handle.released")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_20 {
        all.push((21, kind, json, event));
    }
    all.push((
        21,
        "handle.released",
        format!(
            r#"{{"kind":"handle.released","task":3,"handle":"{}","by":"policy","reason":"handle.policy_released","signed":"none","writers":{{"author":"admin","editors":["write"]}}}}"#,
            hex(12)
        ),
        Event::HandleReleased {
            task: TaskId::from_seq(3),
            handle: d(12),
            by: Resolution::Policy,
            reason: ReasonCode::new("handle.policy_released").ok_or("reason")?,
            signed: Signed::None,
            writers: Some(hyalos_types::Writers {
                author: hyalos_types::Permission::Admin,
                editors: vec![hyalos_types::Permission::Write],
            }),
        },
    ));

    // Version 22 added the `dependency` gate (ADR-0106) and changed no payload, so every kind reads
    // as it did at version 21. Its samples for the two kinds that name a gate are a lost broker's
    // unknown answer and the hold it opens.
    let at_21: Vec<_> = all
        .iter()
        .filter(|g| g.0 == 21 && g.1 != "gate.answered" && g.1 != "hold.opened")
        .cloned()
        .collect();
    for (_, kind, json, event) in at_21 {
        all.push((22, kind, json, event));
    }
    all.push((
        22,
        "gate.answered",
        format!(
            r#"{{"kind":"gate.answered","job":4,"gate":"dependency","inputs":"{}","answer":"unknown","reason":"broker.lost"}}"#,
            hex(13)
        ),
        Event::GateAnswered {
            job: JobId::from_seq(4),
            gate: Gate::Dependency,
            inputs: d(13),
            answer: Answer::Unknown,
            reason: ReasonCode::new("broker.lost").ok_or("reason")?,
        },
    ));
    all.push((
        22,
        "hold.opened",
        format!(
            r#"{{"kind":"hold.opened","task":3,"gate":"dependency","inputs":"{}","reason":"unknown","stakes":{{"tier":"irreversible","touches":[]}}}}"#,
            hex(13)
        ),
        Event::HoldOpened {
            task: TaskId::from_seq(3),
            gate: Gate::Dependency,
            inputs: d(13),
            reason: HoldReason::Unknown,
            stakes: Stakes::irreversible(),
        },
    ));

    // Version 23 added `skill.drafted` (ADR-0113) and changed no payload.
    let at_22: Vec<_> = all.iter().filter(|g| g.0 == 22).cloned().collect();
    for (_, kind, json, event) in at_22 {
        all.push((23, kind, json, event));
    }
    all.push((
        23,
        "skill.drafted",
        format!(
            r#"{{"kind":"skill.drafted","name":"draft-3","digest":"{}","task":3,"grant":{{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":["job->model"]}}}}"#,
            hex(14)
        ),
        Event::SkillDrafted {
            name: hyalos_types::PluginName::new("draft-3").ok_or("name")?,
            digest: d(14),
            task: TaskId::from_seq(3),
            grant: hyalos_types::Grant::read_only(),
        },
    ));

    // Version 24 added `skill.trusted` (ADR-0114) and changed no payload.
    let at_23: Vec<_> = all.iter().filter(|g| g.0 == 23).cloned().collect();
    for (_, kind, json, event) in at_23 {
        all.push((24, kind, json, event));
    }
    all.push((
        24,
        "skill.trusted",
        format!(
            r#"{{"kind":"skill.trusted","name":"draft-3","digest":"{}"}}"#,
            hex(14)
        ),
        Event::SkillTrusted {
            name: hyalos_types::PluginName::new("draft-3").ok_or("name")?,
            digest: d(14),
        },
    ));
    Ok(all)
}

/// The version a kind first appeared at. Before it, the kind cannot have been written.
fn introduced(kind: &str) -> u32 {
    match kind {
        "archive.pruned" => 3,
        "task.cancelled" | "hold.refused" => 4,
        "policy.approved" => 5,
        "handle.released" => 10,
        "agent.issued" | "agent.revoked" => 11,
        "job.values" => 12,
        "merge.approved" => 14,
        "plugin.installed" => 15,
        "plugin.policy_set" => 17,
        "plugin.update_pending" => 18,
        "plugin.snapshot_imported" => 19,
        "plugin.revocations_imported" | "plugin.revoked" => 20,
        "skill.drafted" => 23,
        "skill.trusted" => 24,
        _ => 1,
    }
}

/// Which `(version, kind)` pairs in `1..=current` have no pinned payload.
fn missing(
    golden: &[(u32, &'static str, String, Event)],
    current: u32,
) -> Vec<(u32, &'static str)> {
    let mut out = Vec::new();
    for version in 1..=current {
        for kind in Event::KINDS {
            if version >= introduced(kind) && !golden.iter().any(|g| g.0 == version && g.1 == kind)
            {
                out.push((version, kind));
            }
        }
    }
    out
}

#[test]
fn every_released_version_has_a_pinned_payload_for_every_kind() -> Res {
    assert_eq!(missing(&golden()?, SCHEMA.current), vec![]);
    assert_eq!(SCHEMA.current, SCHEMA_VERSION);
    // The check bites: pretend the next version was released and nobody pinned it.
    assert_eq!(
        missing(&golden()?, SCHEMA.current + 1).len(),
        Event::KINDS.len()
    );
    Ok(())
}

#[test]
fn every_pinned_payload_loads_under_the_current_schema() -> Res {
    for (version, kind, json, event) in golden()? {
        let loaded = Event::decode(kind, version, json.as_bytes())?;
        assert_eq!(loaded, event, "{kind} written at version {version}");
        assert_eq!(loaded.kind(), kind);
    }
    Ok(())
}

#[test]
fn the_pinned_payloads_cover_exactly_the_event_kinds() -> Res {
    let kinds: Vec<&str> = golden()?
        .iter()
        .filter(|g| g.0 == SCHEMA.current)
        .map(|g| g.3.kind())
        .collect();
    for kind in Event::KINDS {
        assert!(kinds.contains(&kind), "{kind} has no sample");
    }
    assert_eq!(kinds.len(), Event::KINDS.len());
    Ok(())
}

// ---- a made-up history: v1 and v2 looked different from today's events (version 3) -----------

fn rename(mut v: Value, from: &str, to: &str) -> Result<Value, String> {
    let object = v.as_object_mut().ok_or("not an object")?;
    let value = object
        .remove(from)
        .ok_or_else(|| format!("no field `{from}`"))?;
    object.insert(to.to_string(), value);
    Ok(v)
}

fn created_v1(v: Value) -> Result<Value, String> {
    rename(v, "digest", "request_digest")
}
fn created_v2(v: Value) -> Result<Value, String> {
    rename(v, "request_digest", "request")
}
fn started_v2(v: Value) -> Result<Value, String> {
    let mut v = rename(v, "task_id", "task")?;
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("substrate".into(), json!("unrecorded"));
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("credentials".into(), json!("unrecorded"));
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("attestation".into(), json!("unrecorded"));
    Ok(v)
}
fn answered_v2(mut v: Value) -> Result<Value, String> {
    // Version 2 did not record why. The default is a constant, as ADR-0023 requires.
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("reason".into(), json!("legacy.unrecorded"));
    Ok(v)
}

static HISTORY: Schema = Schema {
    current: 3,
    steps: &[
        Upcast {
            kind: "task.created",
            from: 1,
            step: created_v1,
        },
        Upcast {
            kind: "task.created",
            from: 2,
            step: created_v2,
        },
        Upcast {
            kind: "job.started",
            from: 2,
            step: started_v2,
        },
        Upcast {
            kind: "gate.answered",
            from: 2,
            step: answered_v2,
        },
    ],
};

fn decode(version: u32, kind: &str, json: &str) -> Result<Event, EventError> {
    Event::decode_with(&HISTORY, kind, version, json.as_bytes())
}

#[test]
fn the_made_up_history_is_a_valid_table() {
    assert_eq!(HISTORY.validate(), Ok(()));
    assert_eq!(SCHEMA.validate(), Ok(()));
}

#[test]
fn a_version_1_event_goes_through_every_step() -> Res {
    let json = format!(r#"{{"kind":"task.created","digest":"{}"}}"#, hex(1));
    assert_eq!(
        decode(1, "task.created", &json)?,
        Event::TaskCreated { request: d(1) }
    );
    Ok(())
}

#[test]
fn a_version_2_event_goes_through_only_the_later_steps() -> Res {
    let json = format!(r#"{{"kind":"task.created","request_digest":"{}"}}"#, hex(1));
    assert_eq!(
        decode(2, "task.created", &json)?,
        Event::TaskCreated { request: d(1) }
    );
    // The version 1 field name is not understood at version 2: steps do not run backwards or twice.
    let v1_shape = format!(r#"{{"kind":"task.created","digest":"{}"}}"#, hex(1));
    assert!(matches!(
        decode(2, "task.created", &v1_shape),
        Err(EventError::Upcast { from: 2, .. })
    ));
    Ok(())
}

#[test]
fn a_kind_with_no_step_at_a_version_passes_through_unchanged() -> Res {
    // job.started has no step from version 1, so its version 1 shape already is its version 2 shape,
    // and the version 2 step then renames the field.
    assert_eq!(
        decode(1, "job.started", r#"{"kind":"job.started","task_id":3}"#)?,
        Event::JobStarted {
            task: TaskId::from_seq(3),
            substrate: SubstrateName::unrecorded(),
            credentials: hyalos_types::CredentialTier::Unrecorded,
            attestation: Attestation::Unrecorded,
        }
    );
    // A kind with no step anywhere is read as stored, at every older version.
    for version in 1..=3 {
        assert_eq!(
            decode(
                version,
                "task.completed",
                r#"{"kind":"task.completed","task":3}"#
            )?,
            Event::TaskCompleted {
                task: TaskId::from_seq(3)
            }
        );
    }
    Ok(())
}

#[test]
fn a_step_can_supply_a_constant_default_but_only_for_older_events() -> Res {
    let without_reason = format!(
        r#"{{"kind":"gate.answered","job":4,"gate":"egress","inputs":"{}","answer":"allow"}}"#,
        hex(6)
    );
    let Event::GateAnswered { reason, .. } = decode(2, "gate.answered", &without_reason)? else {
        return Err("wrong kind".into());
    };
    assert_eq!(reason.as_str(), "legacy.unrecorded");
    // At the current version nothing is filled in: a missing field is an error, never a guess.
    assert!(matches!(
        decode(3, "gate.answered", &without_reason),
        Err(EventError::Decode(_))
    ));
    Ok(())
}

#[test]
fn a_payload_at_the_current_version_is_read_as_stored() -> Res {
    for (_, kind, json, event) in golden()?.into_iter().filter(|g| g.0 == SCHEMA.current) {
        assert_eq!(decode(3, kind, &json)?, event);
    }
    Ok(())
}

#[test]
fn a_step_that_cannot_handle_its_input_names_the_event() {
    let err = decode(1, "task.created", r#"{"kind":"task.created"}"#).unwrap_err();
    assert_eq!(
        err,
        EventError::Upcast {
            kind: "task.created".into(),
            from: 1,
            reason: "no field `digest`".into()
        }
    );
    assert!(err.to_string().contains("`task.created`") && err.to_string().contains("version 1"));
}

#[test]
fn what_an_upcast_leaves_wrong_is_still_refused() {
    // An unknown field survives the steps and the current type refuses it.
    let extra = r#"{"kind":"job.started","task_id":3,"surprise":true}"#;
    assert!(matches!(
        decode(1, "job.started", extra),
        Err(EventError::Decode(_))
    ));
    // Not JSON at all.
    assert!(matches!(
        decode(1, "job.started", "not json"),
        Err(EventError::Decode(_))
    ));
    // A kind column that disagrees with the payload.
    assert!(matches!(
        decode(
            1,
            "task.completed",
            r#"{"kind":"job.started","task":3,"substrate":"local","credentials":"broker_held","attestation":"unrecorded"}"#
        ),
        Err(EventError::KindMismatch { .. })
    ));
}

fn retag(mut v: Value) -> Result<Value, String> {
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("kind".into(), json!("job.started"));
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("substrate".into(), json!("local"));
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("credentials".into(), json!("broker_held"));
    v.as_object_mut()
        .ok_or("not an object")?
        .insert("attestation".into(), json!("unrecorded"));
    Ok(v)
}

static RETAGS: Schema = Schema {
    current: 2,
    steps: &[Upcast {
        kind: "task.completed",
        from: 1,
        step: retag,
    }],
};

#[test]
fn a_step_cannot_change_which_kind_an_event_is() {
    // The log's kind column is hashed, so the tag a step produces must still match it.
    let err = Event::decode_with(
        &RETAGS,
        "task.completed",
        1,
        br#"{"kind":"task.completed","task":3}"#,
    )
    .unwrap_err();
    assert_eq!(
        err,
        EventError::KindMismatch {
            column: "task.completed".into(),
            payload: "job.started"
        }
    );
}

#[test]
fn a_newer_version_than_the_build_knows_is_refused_with_advice() {
    let json = br#"{"kind":"task.completed","task":3}"#;
    for (schema, current) in [(&SCHEMA, SCHEMA.current), (&HISTORY, 3)] {
        let err = Event::decode_with(schema, "task.completed", current + 1, json).unwrap_err();
        assert_eq!(
            err,
            EventError::UnsupportedVersion {
                found: current + 1,
                current
            }
        );
        let text = err.to_string();
        assert!(
            text.contains("newer Hyalos") && text.contains("Upgrade"),
            "{text}"
        );
    }
}

#[test]
fn version_zero_is_never_valid() {
    let json = br#"{"kind":"task.completed","task":3}"#;
    for schema in [&SCHEMA, &HISTORY] {
        let err = Event::decode_with(schema, "task.completed", 0, json).unwrap_err();
        assert_eq!(
            err,
            EventError::UnsupportedVersion {
                found: 0,
                current: schema.current
            }
        );
        assert!(err.to_string().contains("not valid"), "{err}");
    }
}

// ---- a bad table is refused before any event is read -------------------------------------------

fn noop(v: Value) -> Result<Value, String> {
    Ok(v)
}

#[test]
fn a_table_that_contradicts_itself_is_refused() {
    static ZERO: Schema = Schema {
        current: 0,
        steps: &[],
    };
    static UNKNOWN_KIND: Schema = Schema {
        current: 2,
        steps: &[Upcast {
            kind: "task.invented",
            from: 1,
            step: noop,
        }],
    };
    static FROM_ZERO: Schema = Schema {
        current: 2,
        steps: &[Upcast {
            kind: "task.created",
            from: 0,
            step: noop,
        }],
    };
    static FROM_CURRENT: Schema = Schema {
        current: 2,
        steps: &[Upcast {
            kind: "task.created",
            from: 2,
            step: noop,
        }],
    };
    static PAST_CURRENT: Schema = Schema {
        current: 2,
        steps: &[Upcast {
            kind: "task.created",
            from: 5,
            step: noop,
        }],
    };
    static DUPLICATE: Schema = Schema {
        current: 3,
        steps: &[
            Upcast {
                kind: "task.created",
                from: 1,
                step: noop,
            },
            Upcast {
                kind: "task.created",
                from: 1,
                step: noop,
            },
        ],
    };
    for (name, schema) in [
        ("current 0", &ZERO),
        ("unknown kind", &UNKNOWN_KIND),
        ("from 0", &FROM_ZERO),
        ("from current", &FROM_CURRENT),
        ("past current", &PAST_CURRENT),
        ("duplicate", &DUPLICATE),
    ] {
        assert!(schema.validate().is_err(), "{name} was accepted");
    }
}

#[test]
fn the_same_kind_at_different_versions_and_different_kinds_at_one_version_are_fine() {
    static FINE: Schema = Schema {
        current: 3,
        steps: &[
            Upcast {
                kind: "task.created",
                from: 1,
                step: noop,
            },
            Upcast {
                kind: "task.created",
                from: 2,
                step: noop,
            },
            Upcast {
                kind: "job.started",
                from: 1,
                step: noop,
            },
        ],
    };
    assert_eq!(FINE.validate(), Ok(()));
}
