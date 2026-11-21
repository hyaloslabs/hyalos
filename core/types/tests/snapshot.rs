//! The state a snapshot carries, as each released version wrote it (ADR-0026).
//!
//! A snapshot sits in the log for good, signed, so a build must read every version it has ever written.
//! These bytes are what versions 1 to 17 store. Never edit a row: when `STATE_VERSION` is bumped, add the new
//! version's row and the step that brings the old one up to it.

use hyalos_types::snapshot::STATE_VERSION;
use hyalos_types::snapshot::{
    AgentImage, CancelledImage, DraftImage, HandleImage, HoldImage, InstallPolicyImage,
    JobEndImage, JobImage, MergeImage, PendingUpdateImage, PluginImage, PolicyImage, PrunedImage,
    ResolvedImage, RevocationListImage, RevokedImage, RevokedPluginImage, StateImage, TaskImage,
    TrustedRootImage,
};
use hyalos_types::{
    AgentId, AgentName, CommitSha, Digest, Gate, Grant, HoldId, HoldReason, IndexLocation,
    InstallPolicy, JobId, JobOutcome, MergeMethod, PluginName, PluginSource, PluginVersion,
    ReasonCode, Resolution, Sensitive, Stakes, TaskId, Timestamp, TypedValue,
};

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn ms(millis: u64) -> Timestamp {
    Timestamp::from_millis(millis)
}

/// A state with every field set, so a field that is dropped or renamed changes the bytes.
fn everything(
    pruned: Vec<PrunedImage>,
    cancelled: bool,
    policy: bool,
    handles: bool,
    agents: bool,
    job_details: bool,
) -> StateImage {
    let mut image = base(pruned);
    if job_details {
        if let Some(ended) = image.jobs[0].ended.as_mut() {
            ended.reason = ReasonCode::unavailable();
        }
        image.jobs[1].values = Some(vec![TypedValue::Choice(1), TypedValue::Integer(-2)]);
    }
    if agents {
        // A name that did not parse would leave its agent out, which the pinned bytes catch.
        let revoked = Some(RevokedImage {
            at: 5_900,
            by: Resolution::Human,
            reason: ReasonCode::unavailable(),
        });
        image.agents = [
            (22, 5_700, "claude-code", 10, revoked),
            (23, 5_800, "hermes", 11, None),
        ]
        .into_iter()
        .filter_map(|(id, at, name, credential, revoked)| {
            Some(AgentImage {
                id: AgentId::from_seq(id),
                at,
                name: AgentName::new(name)?,
                credential: d(credential),
                by: Resolution::Human,
                reason: ReasonCode::unavailable(),
                revoked,
            })
        })
        .collect();
    }
    if handles {
        image.handles.push(HandleImage {
            seq: 21,
            at: ms(5_600),
            task: TaskId::from_seq(1),
            handle: d(9),
            by: Resolution::Human,
            reason: ReasonCode::unavailable(),
        });
    }
    if policy {
        image.policy = Some(PolicyImage {
            seq: 19,
            at: ms(5_200),
            by: Resolution::Human,
            reason: ReasonCode::unavailable(),
            document: "permit (principal, action, resource);".into(),
        });
    }
    if cancelled {
        // A task that was held and then cancelled; its holds are not open any more.
        image.tasks.push(TaskImage {
            id: TaskId::from_seq(20),
            created_at: 5_000,
            request: d(8),
            completed_at: None,
            open_holds: vec![],
            cancelled: Some(CancelledImage {
                at: 5_500,
                by: Resolution::Human,
                reason: ReasonCode::unavailable(),
            }),
        });
    }
    image
}

fn base(pruned: Vec<PrunedImage>) -> StateImage {
    StateImage {
        last_seq: 17,
        last_at: Some(4_000),
        tasks: vec![
            TaskImage {
                id: TaskId::from_seq(1),
                created_at: 1_000,
                request: d(1),
                completed_at: None,
                open_holds: vec![HoldId::from_seq(5)],
                cancelled: None,
            },
            TaskImage {
                id: TaskId::from_seq(8),
                created_at: 2_000,
                request: d(3),
                completed_at: Some(3_000),
                open_holds: vec![],
                cancelled: None,
            },
        ],
        jobs: vec![
            JobImage {
                id: JobId::from_seq(2),
                task: TaskId::from_seq(1),
                started_at: 1_500,
                ended: Some(JobEndImage {
                    at: 1_800,
                    outcome: JobOutcome::Held,
                    reason: ReasonCode::new("legacy.unrecorded")
                        .unwrap_or_else(ReasonCode::unavailable),
                }),
                values: None,
            },
            JobImage {
                id: JobId::from_seq(7),
                task: TaskId::from_seq(6),
                started_at: 2_500,
                ended: None,
                values: None,
            },
        ],
        holds: vec![
            HoldImage {
                id: HoldId::from_seq(5),
                task: TaskId::from_seq(1),
                gate: Gate::Egress,
                inputs: d(2),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible().touching(Sensitive::Credentials),
                opened_at: 1_800,
                resolved: None,
            },
            HoldImage {
                id: HoldId::from_seq(16),
                task: TaskId::from_seq(12),
                gate: Gate::Content,
                inputs: d(4),
                reason: HoldReason::TimedOut,
                stakes: Stakes::routine(),
                opened_at: 3_500,
                resolved: Some(ResolvedImage {
                    at: 3_900,
                    by: Resolution::Policy,
                    reason: ReasonCode::unavailable(),
                }),
            },
        ],
        pruned,
        policy: None,
        handles: vec![],
        agents: vec![],
        last_approval: None,
        signed_since: None,
        merges: vec![],
        plugins: vec![],
        install_policy: None,
        pending_updates: vec![],
        trusted_root: None,
        revocations: None,
        revoked_plugins: vec![],
        drafts: vec![],
    }
}

fn golden_pruned() -> Vec<PrunedImage> {
    vec![
        PrunedImage {
            seq: 10,
            at: ms(4_100),
            last_seq: 3,
            last_hash: d(5),
        },
        PrunedImage {
            seq: 18,
            at: ms(4_800),
            last_seq: 9,
            last_hash: d(6),
        },
    ]
}

/// The state version 9 stored: every field it had set.
fn v9_image() -> StateImage {
    let mut image = everything(Vec::new(), true, true, true, true, true);
    image.pruned = golden_pruned();
    image.last_approval = Some(21);
    image.signed_since = Some(19);
    image.merges.push(MergeImage {
        seq: 24,
        at: ms(6_000),
        task: TaskId::from_seq(20),
        repo: "octo/repo".into(),
        pr: 42,
        head: CommitSha::new("0123456789abcdef0123456789abcdef01234567")
            .unwrap_or_else(|| unreachable!()),
        method: MergeMethod::Squash,
        by: Resolution::Human,
        reason: ReasonCode::unavailable(),
    });
    image
}

/// `(version, the bytes that version stored, the state they mean)`.
fn golden() -> Vec<(u32, String, StateImage)> {
    let pruned = vec![
        PrunedImage {
            seq: 10,
            at: ms(4_100),
            last_seq: 3,
            last_hash: d(5),
        },
        PrunedImage {
            seq: 18,
            at: ms(4_800),
            last_seq: 9,
            last_hash: d(6),
        },
    ];
    vec![
        // Version 1 had no list of pruned archives; it reads as an empty one.
        (
            1,
            PINNED_V1.to_string(),
            everything(vec![], false, false, false, false, false),
        ),
        // Version 2 had no cancelled tasks; they read as none.
        (
            2,
            PINNED_V2.to_string(),
            everything(pruned.clone(), false, false, false, false, false),
        ),
        (
            3,
            PINNED_V3.to_string(),
            everything(pruned.clone(), true, false, false, false, false),
        ),
        // Version 4 added the operator policy in force; the older versions read as having none.
        (
            4,
            PINNED_V4.to_string(),
            everything(pruned.clone(), true, true, false, false, false),
        ),
        // Version 5 added the handles released; the older versions read as having none.
        (
            5,
            PINNED_V5.to_string(),
            everything(pruned.clone(), true, true, true, false, false),
        ),
        // Version 6 added the agents' credentials; the older versions read as having none.
        (
            6,
            PINNED_V6.to_string(),
            everything(pruned.clone(), true, true, true, true, false),
        ),
        // Version 7 added why each job ended and the values it returned; older jobs read as ended for
        // an unrecorded reason, with no values.
        (
            7,
            PINNED_V7.to_string(),
            everything(pruned.clone(), true, true, true, true, true),
        ),
        // Version 8 added the last decision the operator signed and the first event written at schema
        // version 13; the older versions read as having neither.
        (8, PINNED_V8.to_string(), {
            let mut image = everything(pruned, true, true, true, true, true);
            image.last_approval = Some(21);
            image.signed_since = Some(19);
            image
        }),
        // Version 9 added the merges approved; the older versions read as having none.
        (9, PINNED_V9.to_string(), v9_image()),
        // Version 10 added the plugins installed; the older versions read as having none.
        (10, PINNED_V10.to_string(), v10_image()),
        // Version 11 added an index as where a plugin came from; version 10 reads as it was.
        (11, PINNED_V11.to_string(), v11_image()),
        // Version 12 added the install policy; the older versions read as having none.
        (12, PINNED_V12.to_string(), v12_image()),
        // Version 13 added the updates waiting; the older versions read as having none.
        (13, PINNED_V13.to_string(), v13_image()),
        // Version 14 added the trusted-root snapshot imported; the older versions read as having none.
        (14, PINNED_V14.to_string(), v14_image()),
        // Version 15 added the revocation list imported and the plugins it revoked; the older versions
        // read as having neither.
        (15, PINNED_V15.to_string(), v15_image()),
        // Version 16 added the skills Hyalos drafted; the older versions read as having none.
        (16, PINNED_V16.to_string(), v16_image()),
        // Version 17 added when each draft was trusted; an older draft reads as not trusted.
        (17, PINNED_V17.to_string(), {
            let mut image = v16_image();
            image.drafts[0].trusted = Some(33);
            image
        }),
    ]
}

fn v16_image() -> StateImage {
    {
        let mut image = v15_image();
        image.drafts.push(DraftImage {
            seq: 32,
            at: ms(6_800),
            name: PluginName::new("draft-8").unwrap_or_else(|| unreachable!()),
            digest: d(17),
            task: TaskId::from_seq(8),
            grant: Grant::read_only(),
            trusted: None,
        });
        image
    }
}

fn v15_image() -> StateImage {
    {
        let mut image = v14_image();
        image.revocations = Some(RevocationListImage {
            seq: 30,
            at: ms(6_600),
            digest: d(16),
            signed_at: ms(1_794_882_145_000),
        });
        image.revoked_plugins.push(RevokedPluginImage {
            seq: 31,
            at: ms(6_700),
            name: PluginName::new("pdf-tools").unwrap_or_else(|| unreachable!()),
            digest: d(12),
            reason: ReasonCode::new("malware.exfiltration").unwrap_or_else(|| unreachable!()),
        });
        image
    }
}

fn v14_image() -> StateImage {
    let mut image = v13_image();
    image.trusted_root = Some(TrustedRootImage {
        seq: 29,
        at: ms(6_500),
        digest: d(15),
        signed_at: ms(1_794_878_463_000),
    });
    image
}

fn v13_image() -> StateImage {
    {
        let mut image = v12_image();
        image.pending_updates.push(PendingUpdateImage {
            seq: 28,
            at: ms(6_400),
            name: PluginName::new("lint").unwrap_or_else(|| unreachable!()),
            version: PluginVersion::new("2.0.0").unwrap_or_else(|| unreachable!()),
            digest: d(14),
            asks: Grant {
                actions: ["push".to_string()].into(),
                ..Grant::none()
            },
            location: IndexLocation::new("https://plugins.example/v1")
                .unwrap_or_else(|| unreachable!()),
        });
        image
    }
}

fn v12_image() -> StateImage {
    {
        let mut image = v11_image();
        image.install_policy = Some(InstallPolicyImage {
            seq: 27,
            at: ms(6_300),
            policy: InstallPolicy {
                ceiling: Grant {
                    egress_hosts: ["docs.rs".to_string()].into(),
                    ..Grant::none()
                },
                indexes: [IndexLocation::new("https://plugins.example/v1")
                    .unwrap_or_else(|| unreachable!())]
                .into(),
                max_snapshot_age_days: 90,
                max_revocation_age_days: 30,
            },
            by: Resolution::Human,
            reason: ReasonCode::unavailable(),
        });
        image
    }
}

fn v11_image() -> StateImage {
    {
        let mut image = v10_image();
        image.plugins.push(PluginImage {
            seq: 26,
            at: ms(6_200),
            name: PluginName::new("lint").unwrap_or_else(|| unreachable!()),
            digest: d(13),
            grant: Grant::none(),
            source: PluginSource::Index {
                location: IndexLocation::new("https://plugins.example/v1")
                    .unwrap_or_else(|| unreachable!()),
                version: PluginVersion::new("1.2.0").unwrap_or_else(|| unreachable!()),
                signed_at: None,
            },
            by: Resolution::Human,
            reason: ReasonCode::unavailable(),
        });
        image
    }
}

fn v10_image() -> StateImage {
    {
        let mut image = v9_image();
        image.plugins.push(PluginImage {
            seq: 25,
            at: ms(6_100),
            name: PluginName::new("pdf-tools").unwrap_or_else(|| unreachable!()),
            digest: d(12),
            grant: Grant {
                actions: ["read".to_string()].into(),
                egress_hosts: ["docs.rs".to_string()].into(),
                data_flows: ["job->web".to_string()].into(),
                ..Grant::none()
            },
            source: PluginSource::Local,
            by: Resolution::Human,
            reason: ReasonCode::unavailable(),
        });
        image
    }
}

const PINNED_V1: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5]},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[]}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}]}"#;

#[test]
fn every_released_version_still_reads() {
    for (version, bytes, image) in golden() {
        assert_eq!(
            StateImage::decode(version, bytes.as_bytes()),
            Ok(image),
            "version {version}"
        );
    }
}

#[test]
fn the_current_version_writes_exactly_the_pinned_bytes() {
    let (_, bytes, image) = golden()
        .into_iter()
        .find(|(v, _, _)| *v == STATE_VERSION)
        .expect("the current version must have a pinned row");
    assert_eq!(
        String::from_utf8(image.encode().expect("encodes")).expect("utf8"),
        bytes
    );
}

#[test]
fn every_version_up_to_the_current_has_a_pinned_row() {
    let versions: Vec<u32> = golden().iter().map(|(v, _, _)| *v).collect();
    assert_eq!(versions, (1..=STATE_VERSION).collect::<Vec<_>>());
}

const PINNED_V2: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5]},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[]}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}]}"#;

const PINNED_V4: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"}}"#;

const PINNED_V5: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}]}"#;

const PINNED_V6: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}]}"#;

const PINNED_V3: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}]}"#;

/// A replay knows why each job ended and what it returned; a snapshot written before version 7 did
/// not keep either, so an audit compares the replay as that version keeps it (ADR-0069).
#[test]
fn a_replay_as_an_old_version_keeps_it_matches_that_versions_snapshot() {
    let replay = everything(Vec::new(), true, true, true, true, true);
    let old = everything(Vec::new(), true, true, true, true, false);
    assert!(replay.first_difference(&old).is_some());
    assert_eq!(replay.clone().as_kept_at(6).first_difference(&old), None);
    assert_eq!(replay.clone().as_kept_at(8), replay);
    let signed = StateImage {
        last_approval: Some(21),
        signed_since: Some(19),
        ..replay.clone()
    };
    assert_eq!(signed.as_kept_at(7), replay);
}

#[test]
fn an_old_image_is_refused_if_it_is_damaged_not_just_old() {
    // The step that supplies the empty list does not hide a field it does not know.
    let bad = PINNED_V1.replacen("{\"last_seq\"", "{\"surprise\":1,\"last_seq\"", 1);
    assert!(StateImage::decode(1, bad.as_bytes()).is_err());
    assert!(StateImage::decode(1, b"[]").is_err());
    // A version 2 image must carry the list: it is not optional once the step has run.
    assert!(StateImage::decode(2, PINNED_V1.as_bytes()).is_err());
}

const PINNED_V9: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}]}"#;

const PINNED_V13: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":27,"at":6300,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"},"pending_updates":[{"seq":28,"at":6400,"name":"lint","version":"2.0.0","digest":"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e","asks":{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]},"location":"https://plugins.example/v1"}]}"#;

const PINNED_V14: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":27,"at":6300,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"},"pending_updates":[{"seq":28,"at":6400,"name":"lint","version":"2.0.0","digest":"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e","asks":{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]},"location":"https://plugins.example/v1"}],"trusted_root":{"seq":29,"at":6500,"digest":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","signed_at":1794878463000}}"#;

const PINNED_V15: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":27,"at":6300,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"},"pending_updates":[{"seq":28,"at":6400,"name":"lint","version":"2.0.0","digest":"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e","asks":{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]},"location":"https://plugins.example/v1"}],"trusted_root":{"seq":29,"at":6500,"digest":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","signed_at":1794878463000},"revocations":{"seq":30,"at":6600,"digest":"1010101010101010101010101010101010101010101010101010101010101010","signed_at":1794882145000},"revoked_plugins":[{"seq":31,"at":6700,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","reason":"malware.exfiltration"}]}"#;

const PINNED_V12: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":27,"at":6300,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"}}"#;

const PINNED_V11: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}]}"#;

const PINNED_V10: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"}]}"#;

const PINNED_V7: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}]}"#;

const PINNED_V8: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19}"#;

const PINNED_V16: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":27,"at":6300,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"},"pending_updates":[{"seq":28,"at":6400,"name":"lint","version":"2.0.0","digest":"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e","asks":{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]},"location":"https://plugins.example/v1"}],"trusted_root":{"seq":29,"at":6500,"digest":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","signed_at":1794878463000},"revocations":{"seq":30,"at":6600,"digest":"1010101010101010101010101010101010101010101010101010101010101010","signed_at":1794882145000},"revoked_plugins":[{"seq":31,"at":6700,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","reason":"malware.exfiltration"}],"drafts":[{"seq":32,"at":6800,"name":"draft-8","digest":"1111111111111111111111111111111111111111111111111111111111111111","task":8,"grant":{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":["job->model"]}}]}"#;

const PINNED_V17: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held","reason":"gate.unavailable"},"values":null},{"id":7,"task":6,"started_at":2500,"ended":null,"values":[{"choice":1},{"integer":-2}]}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}],"policy":{"seq":19,"at":5200,"by":"human","reason":"gate.unavailable","document":"permit (principal, action, resource);"},"handles":[{"seq":21,"at":5600,"task":1,"handle":"0909090909090909090909090909090909090909090909090909090909090909","by":"human","reason":"gate.unavailable"}],"agents":[{"id":22,"at":5700,"name":"claude-code","credential":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","by":"human","reason":"gate.unavailable","revoked":{"at":5900,"by":"human","reason":"gate.unavailable"}},{"id":23,"at":5800,"name":"hermes","credential":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","by":"human","reason":"gate.unavailable","revoked":null}],"last_approval":21,"signed_since":19,"merges":[{"seq":24,"at":6000,"task":20,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"gate.unavailable"}],"plugins":[{"seq":25,"at":6100,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","grant":{"actions":["read"],"egress_hosts":["docs.rs"],"tools":[],"data_flows":["job->web"]},"source":"local","by":"human","reason":"gate.unavailable"},{"seq":26,"at":6200,"name":"lint","digest":"0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d","grant":{"actions":[],"egress_hosts":[],"tools":[],"data_flows":[]},"source":{"index":{"location":"https://plugins.example/v1","version":"1.2.0"}},"by":"human","reason":"gate.unavailable"}],"install_policy":{"seq":27,"at":6300,"policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"gate.unavailable"},"pending_updates":[{"seq":28,"at":6400,"name":"lint","version":"2.0.0","digest":"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e","asks":{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]},"location":"https://plugins.example/v1"}],"trusted_root":{"seq":29,"at":6500,"digest":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","signed_at":1794878463000},"revocations":{"seq":30,"at":6600,"digest":"1010101010101010101010101010101010101010101010101010101010101010","signed_at":1794882145000},"revoked_plugins":[{"seq":31,"at":6700,"name":"pdf-tools","digest":"0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c","reason":"malware.exfiltration"}],"drafts":[{"seq":32,"at":6800,"name":"draft-8","digest":"1111111111111111111111111111111111111111111111111111111111111111","task":8,"grant":{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":["job->model"]},"trusted":33}]}"#;
