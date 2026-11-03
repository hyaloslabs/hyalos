//! The state a snapshot carries, as each released version wrote it (ADR-0026).
//!
//! A snapshot sits in the log for good, signed, so a build must read every version it has ever written.
//! These bytes are what versions 1, 2 and 3 store. Never edit a row: when `STATE_VERSION` is bumped, add the new
//! version's row and the step that brings the old one up to it.

use hyalos_types::snapshot::STATE_VERSION;
use hyalos_types::snapshot::{
    CancelledImage, HoldImage, JobEndImage, JobImage, PrunedImage, ResolvedImage, StateImage,
    TaskImage,
};
use hyalos_types::{
    Digest, Gate, HoldId, HoldReason, JobId, JobOutcome, ReasonCode, Resolution, Sensitive, Stakes,
    TaskId,
};

fn d(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

/// A state with every field set, so a field that is dropped or renamed changes the bytes.
fn everything(pruned: Vec<PrunedImage>, cancelled: bool) -> StateImage {
    let mut image = base(pruned);
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
                }),
            },
            JobImage {
                id: JobId::from_seq(7),
                task: TaskId::from_seq(6),
                started_at: 2_500,
                ended: None,
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
    }
}

/// `(version, the bytes that version stored, the state they mean)`.
fn golden() -> Vec<(u32, String, StateImage)> {
    let pruned = vec![
        PrunedImage {
            seq: 10,
            at: 4_100,
            last_seq: 3,
            last_hash: d(5),
        },
        PrunedImage {
            seq: 18,
            at: 4_800,
            last_seq: 9,
            last_hash: d(6),
        },
    ];
    vec![
        // Version 1 had no list of pruned archives; it reads as an empty one.
        (1, PINNED_V1.to_string(), everything(vec![], false)),
        // Version 2 had no cancelled tasks; they read as none.
        (2, PINNED_V2.to_string(), everything(pruned.clone(), false)),
        (3, PINNED_V3.to_string(), everything(pruned, true)),
    ]
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

const PINNED_V3: &str = r#"{"last_seq":17,"last_at":4000,"tasks":[{"id":1,"created_at":1000,"request":"0101010101010101010101010101010101010101010101010101010101010101","completed_at":null,"open_holds":[5],"cancelled":null},{"id":8,"created_at":2000,"request":"0303030303030303030303030303030303030303030303030303030303030303","completed_at":3000,"open_holds":[],"cancelled":null},{"id":20,"created_at":5000,"request":"0808080808080808080808080808080808080808080808080808080808080808","completed_at":null,"open_holds":[],"cancelled":{"at":5500,"by":"human","reason":"gate.unavailable"}}],"jobs":[{"id":2,"task":1,"started_at":1500,"ended":{"at":1800,"outcome":"held"}},{"id":7,"task":6,"started_at":2500,"ended":null}],"holds":[{"id":5,"task":1,"gate":"egress","inputs":"0202020202020202020202020202020202020202020202020202020202020202","reason":"unknown","stakes":{"tier":"irreversible","touches":["credentials"]},"opened_at":1800,"resolved":null},{"id":16,"task":12,"gate":"content","inputs":"0404040404040404040404040404040404040404040404040404040404040404","reason":"timed_out","stakes":{"tier":"routine","touches":[]},"opened_at":3500,"resolved":{"at":3900,"by":"policy","reason":"gate.unavailable"}}],"pruned":[{"seq":10,"at":4100,"last_seq":3,"last_hash":"0505050505050505050505050505050505050505050505050505050505050505"},{"seq":18,"at":4800,"last_seq":9,"last_hash":"0606060606060606060606060606060606060606060606060606060606060606"}]}"#;

#[test]
fn an_old_image_is_refused_if_it_is_damaged_not_just_old() {
    // The step that supplies the empty list does not hide a field it does not know.
    let bad = PINNED_V1.replacen("{\"last_seq\"", "{\"surprise\":1,\"last_seq\"", 1);
    assert!(StateImage::decode(1, bad.as_bytes()).is_err());
    assert!(StateImage::decode(1, b"[]").is_err());
    // A version 2 image must carry the list: it is not optional once the step has run.
    assert!(StateImage::decode(2, PINNED_V1.as_bytes()).is_err());
}
