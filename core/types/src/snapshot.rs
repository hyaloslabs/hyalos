//! The state a snapshot carries (ADR-0026).
//!
//! Compaction replaces events with a signed snapshot of the state they led to. The log signs the bytes
//! without reading them; this is the layout of those bytes. Like events they are typed values,
//! digests and references, never secrets, and decoding refuses unknown fields and unknown versions.
//!
//! The image mirrors the engine's state field for field. It is a separate type so the engine's state
//! can change shape without its stored form changing silently: when the image changes, [`STATE_VERSION`]
//! is bumped and a step is added, as for events (ADR-0023).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, AgentName, CommitSha, Digest, Gate, Grant, HoldId, HoldReason, IndexLocation,
    InstallPolicy, JobId, JobOutcome, MergeMethod, PluginName, PluginSource, PluginVersion,
    ReasonCode, Resolution, Stakes, TaskId, TypedValue, Value,
};

/// The layout [`StateImage`] is written in today.
///
/// Version 2 added the archives that were pruned (ADR-0027). A version 1 image has none. Version 3 added
/// the cancelling of a task (ADR-0029). Earlier images have no task cancelled. Version 4 added the policy
/// in force (ADR-0034). Earlier images have none, which is the baseline alone. Version 5 added the
/// handles released (ADR-0063). Earlier images have none. Version 6 added the agents' credentials
/// (ADR-0068). Earlier images have none. Version 7 added why each job ended and the values it returned
/// (ADR-0069). An earlier job ended for an unrecorded reason and returned none. Version 8 added the
/// last decision the operator signed and the first event at the signed schema version (ADR-0070).
/// Version 9 added the merges approved (ADR-0071); earlier images have none. Version 10 added the
/// plugins installed (ADR-0089); earlier images have none. Version 11 added a plugin index as where
/// an installed plugin came from (ADR-0090); an earlier image holds only local installs, which read as
/// they were. Version 12 added the install policy (ADR-0091); earlier images have none. Version 13
/// added the updates waiting for the operator (ADR-0092); earlier images have none. Version 14 added
/// the trusted-root snapshot imported last (ADR-0097); earlier images have none, so the embedded one
/// is in force. Version 15 added the revocation list imported last and the installed plugins it
/// revoked (ADR-0098); earlier images have neither. Version 16 added the skills Hyalos drafted
/// (ADR-0113); earlier images have none.
pub const STATE_VERSION: u32 = 16;

/// Every task, job and hold as of `last_seq`. Lists are in id order, so one state has one encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateImage {
    /// The last event the state covers, which is the event the snapshot stands for.
    pub last_seq: u64,
    /// The stamp of that event, in milliseconds, if there was one.
    pub last_at: Option<u64>,
    pub tasks: Vec<TaskImage>,
    pub jobs: Vec<JobImage>,
    pub holds: Vec<HoldImage>,
    /// Every archive pruned so far, in the order it was recorded. It lives in the snapshot so that
    /// compacting again does not take the record out of the live log.
    pub pruned: Vec<PrunedImage>,
    /// The operator policy in force, from the latest `policy.approved` event. `None` until one is
    /// approved, when only the shipped baseline applies (ADR-0034).
    pub policy: Option<PolicyImage>,
    /// Every handle released, in the order it was recorded (ADR-0063).
    pub handles: Vec<HandleImage>,
    /// Every agent's credential issued, revoked ones too, in id order (ADR-0068).
    pub agents: Vec<AgentImage>,
    /// The event of the last decision the operator signed. The next one must have been signed after
    /// seeing it, so an old approval cannot be written again (ADR-0070).
    pub last_approval: Option<u64>,
    /// The first event written at schema version 13 or later: from it on, a human decision without
    /// the operator's signature is refused (ADR-0070).
    pub signed_since: Option<u64>,
    /// Every merge approved, in the order it was recorded (ADR-0071).
    pub merges: Vec<MergeImage>,
    /// Every plugin installed, the latest install of each name, in name order (ADR-0089).
    pub plugins: Vec<PluginImage>,
    /// The install policy in force, if the operator set one (ADR-0091).
    pub install_policy: Option<InstallPolicyImage>,
    /// Each update an update check found waiting for the operator, by name (ADR-0092).
    pub pending_updates: Vec<PendingUpdateImage>,
    /// The trusted-root snapshot imported last, if one was (ADR-0097).
    pub trusted_root: Option<TrustedRootImage>,
    /// The revocation list imported last, if one was (ADR-0098).
    pub revocations: Option<RevocationListImage>,
    /// Each installed plugin whose release is revoked, by name (ADR-0098).
    pub revoked_plugins: Vec<RevokedPluginImage>,
    /// Each skill Hyalos drafted from a run that succeeded, by name (ADR-0113).
    pub drafts: Vec<DraftImage>,
}

/// A `skill.drafted` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftImage {
    /// The sequence number of the event that recorded it.
    pub seq: u64,
    pub at: u64,
    pub name: PluginName,
    pub digest: Digest,
    pub task: TaskId,
    pub grant: Grant,
}

/// The latest `plugin.revocations_imported` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationListImage {
    /// The sequence number of the event that imported it.
    pub seq: u64,
    pub at: u64,
    pub digest: Digest,
    /// Its issue time, in milliseconds.
    pub signed_at: u64,
}

/// A `plugin.revoked` event for the release installed under its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokedPluginImage {
    /// The sequence number of the event that recorded it.
    pub seq: u64,
    pub at: u64,
    pub name: PluginName,
    pub digest: Digest,
    pub reason: ReasonCode,
}

/// The latest `plugin.snapshot_imported` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedRootImage {
    /// The sequence number of the event that imported it.
    pub seq: u64,
    pub at: u64,
    pub digest: Digest,
    /// Its issue time, in milliseconds.
    pub signed_at: u64,
}

/// The latest `plugin.update_pending` event for one name, not yet installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingUpdateImage {
    /// The sequence number of the event that found it.
    pub seq: u64,
    pub at: u64,
    pub name: PluginName,
    pub version: PluginVersion,
    pub digest: Digest,
    pub asks: Grant,
    pub location: IndexLocation,
}

/// The latest `plugin.policy_set` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallPolicyImage {
    /// The sequence number of the event that set it.
    pub seq: u64,
    pub at: u64,
    pub policy: InstallPolicy,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// The latest `plugin.installed` event for one name: which files a job may be given under that name,
/// and the grant that bounds the job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginImage {
    /// The sequence number of the event that installed it.
    pub seq: u64,
    pub at: u64,
    pub name: PluginName,
    pub digest: Digest,
    pub grant: Grant,
    pub source: PluginSource,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// A `merge.approved` event, kept: a merge job's manifest is minted only from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeImage {
    /// The sequence number of the event that approved it.
    pub seq: u64,
    pub at: u64,
    pub task: TaskId,
    pub repo: String,
    pub pr: u64,
    pub head: CommitSha,
    pub method: MergeMethod,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// An `agent.issued` event and, if one followed, its `agent.revoked`: whether the MCP server admits
/// the credential depends on both, so both travel with the state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentImage {
    pub id: AgentId,
    pub at: u64,
    pub name: AgentName,
    pub credential: Digest,
    pub by: Resolution,
    pub reason: ReasonCode,
    pub revoked: Option<RevokedImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokedImage {
    pub at: u64,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// A `handle.released` event, kept: a manifest may name the handle only while its task stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandleImage {
    /// The sequence number of the event that released it.
    pub seq: u64,
    pub at: u64,
    pub task: TaskId,
    pub handle: Digest,
    pub by: Resolution,
    pub reason: ReasonCode,
}

/// A `policy.approved` event, kept: the document is the policy in force, so it travels with the state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyImage {
    /// The sequence number of the event that approved it.
    pub seq: u64,
    pub at: u64,
    pub by: Resolution,
    pub reason: ReasonCode,
    pub document: String,
}

/// An `archive.pruned` event, kept (ADR-0026).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrunedImage {
    /// The sequence number of the event that recorded it.
    pub seq: u64,
    pub at: u64,
    /// The last event the archive held, and its hash.
    pub last_seq: u64,
    pub last_hash: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskImage {
    pub id: TaskId,
    pub created_at: u64,
    pub request: Digest,
    pub completed_at: Option<u64>,
    /// In id order.
    pub open_holds: Vec<HoldId>,
    /// Set when the task was cancelled while held. A cancelled task has no open holds, and the holds
    /// that were open stay unresolved: they were ended, not released.
    pub cancelled: Option<CancelledImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelledImage {
    pub at: u64,
    pub by: Resolution,
    pub reason: ReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobImage {
    pub id: JobId,
    pub task: TaskId,
    pub started_at: u64,
    pub ended: Option<JobEndImage>,
    pub values: Option<Vec<TypedValue>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobEndImage {
    pub at: u64,
    pub outcome: JobOutcome,
    pub reason: ReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldImage {
    pub id: HoldId,
    pub task: TaskId,
    pub gate: Gate,
    pub inputs: Digest,
    pub reason: HoldReason,
    pub stakes: Stakes,
    pub opened_at: u64,
    pub resolved: Option<ResolvedImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedImage {
    pub at: u64,
    pub by: Resolution,
    pub reason: ReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Written by a build this one does not know: a newer one, or no version at all. Never skipped.
    UnsupportedVersion {
        found: u32,
        current: u32,
    },
    Encode(String),
    Decode(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::UnsupportedVersion { found, current } => write!(
                f,
                "the snapshot's state is version {found} but this Hyalos reads up to version {current}: it was written by a newer Hyalos (or is damaged). Upgrade to read it"
            ),
            ImageError::Encode(e) => write!(f, "cannot encode the state: {e}"),
            ImageError::Decode(e) => write!(f, "cannot read the snapshot's state: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl StateImage {
    /// The bytes a snapshot stores, at [`STATE_VERSION`].
    pub fn encode(&self) -> Result<Vec<u8>, ImageError> {
        serde_json::to_vec(self).map_err(|e| ImageError::Encode(e.to_string()))
    }

    /// The first field where `self` and `other` differ, as a path and the two values, or `None` if they
    /// are equal. Fields are compared in alphabetical order and lists by position, naming an entry by
    /// its id, so the answer is the same every time. `self` is called the replay and `other`
    /// the snapshot in the text, which is how an audit uses it.
    pub fn first_difference(&self, other: &StateImage) -> Option<String> {
        let (a, b) = (
            serde_json::to_value(self).ok()?,
            serde_json::to_value(other).ok()?,
        );
        difference("state", &a, &b)
    }

    /// This state as an image written at `version` holds it: what that version did not keep reads as
    /// [`StateImage::decode`] fills it in. An audit compares a replay with an old snapshot this way,
    /// since the replay knows what the snapshot never recorded (ADR-0069).
    pub fn as_kept_at(mut self, version: u32) -> Self {
        // Before 7 no job's reason or values were kept. The earlier versions only added what could not
        // have happened yet, so a replay has nothing more to drop for them.
        if version < 8 {
            self.last_approval = None;
            self.signed_since = None;
        }
        if version < 7 {
            for job in &mut self.jobs {
                job.values = None;
                if let Some(ended) = job.ended.as_mut() {
                    ended.reason =
                        ReasonCode::new("legacy.unrecorded").unwrap_or(ended.reason.clone());
                }
            }
        }
        self
    }

    /// Reads bytes written at `version`, bringing an older one up to date.
    pub fn decode(version: u32, bytes: &[u8]) -> Result<Self, ImageError> {
        if version == 0 || version > STATE_VERSION {
            return Err(ImageError::UnsupportedVersion {
                found: version,
                current: STATE_VERSION,
            });
        }
        if version == STATE_VERSION {
            return serde_json::from_slice(bytes).map_err(|e| ImageError::Decode(e.to_string()));
        }
        let mut value: Value =
            serde_json::from_slice(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
        // 1 -> 2: nothing had been pruned.
        if version == 1 {
            value
                .as_object_mut()
                .ok_or_else(|| ImageError::Decode("the state is not an object".into()))?
                .insert("pruned".into(), Value::Array(Vec::new()));
        }
        // 2 -> 3: no task had been cancelled.
        if version < 3 {
            let tasks = value
                .get_mut("tasks")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| ImageError::Decode("the state has no list of tasks".into()))?;
            for task in tasks {
                task.as_object_mut()
                    .ok_or_else(|| ImageError::Decode("a task is not an object".into()))?
                    .insert("cancelled".into(), Value::Null);
            }
        }
        let object = value
            .as_object_mut()
            .ok_or_else(|| ImageError::Decode("the state is not an object".into()))?;
        // 3 -> 4: no operator policy had been approved.
        if version < 4 {
            object.insert("policy".into(), Value::Null);
        }
        // 4 -> 5: no handle had been released.
        if version < 5 {
            object.insert("handles".into(), Value::Array(Vec::new()));
        }
        // 5 -> 6: no agent's credential had been issued.
        if version < 6 {
            object.insert("agents".into(), Value::Array(Vec::new()));
        }
        // 7 -> 8: no decision had been signed by the operator.
        if version < 8 {
            object.insert("last_approval".into(), Value::Null);
            object.insert("signed_since".into(), Value::Null);
        }
        // 8 -> 9: no merge had been approved.
        if version < 9 {
            object.insert("merges".into(), Value::Array(Vec::new()));
        }
        // 9 -> 10: no plugin had been installed. 10 -> 11 changes nothing stored.
        if version < 10 {
            object.insert("plugins".into(), Value::Array(Vec::new()));
        }
        // 11 -> 12: no install policy had been set.
        if version < 12 {
            object.insert("install_policy".into(), Value::Null);
        }
        // 12 -> 13: no update was waiting.
        if version < 13 {
            object.insert("pending_updates".into(), Value::Array(Vec::new()));
        }
        // 13 -> 14: no trusted-root snapshot had been imported.
        if version < 14 {
            object.insert("trusted_root".into(), Value::Null);
        }
        // 14 -> 15: no revocation list had been imported, so nothing installed was revoked.
        if version < 15 {
            object.insert("revocations".into(), Value::Null);
            object.insert("revoked_plugins".into(), Value::Array(Vec::new()));
        }
        // 15 -> 16: no skill had been drafted.
        if version < 16 {
            object.insert("drafts".into(), Value::Array(Vec::new()));
        }
        // 6 -> 7: no job's reason was kept, and no job had returned values.
        if version >= 7 {
            return serde_json::from_value(value).map_err(|e| ImageError::Decode(e.to_string()));
        }
        let jobs = object
            .get_mut("jobs")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| ImageError::Decode("the state has no list of jobs".into()))?;
        for job in jobs {
            let job = job
                .as_object_mut()
                .ok_or_else(|| ImageError::Decode("a job is not an object".into()))?;
            job.insert("values".into(), Value::Null);
            if let Some(ended) = job.get_mut("ended").and_then(Value::as_object_mut) {
                ended.insert("reason".into(), Value::String("legacy.unrecorded".into()));
            }
        }
        serde_json::from_value(value).map_err(|e| ImageError::Decode(e.to_string()))
    }
}

fn difference(path: &str, replay: &Value, snapshot: &Value) -> Option<String> {
    if replay == snapshot {
        return None;
    }
    match (replay, snapshot) {
        (Value::Object(a), Value::Object(b)) => {
            // Alphabetical, sorted here and not left to serde_json: a dependency anywhere in the build
            // can turn on its `preserve_order` feature (cedar-policy does), which changes the order a
            // map is walked in for every crate, and the first difference must not depend on that.
            let mut keys: Vec<&String> = a.keys().collect();
            keys.sort();
            for key in keys {
                let Some(x) = a.get(key) else { continue };
                match b.get(key) {
                    Some(y) => {
                        if let Some(found) = difference(&format!("{path}.{key}"), x, y) {
                            return Some(found);
                        }
                    }
                    None => return Some(format!("{path}.{key}: only the replay has it")),
                }
            }
            let mut extra: Vec<&String> = b.keys().filter(|k| !a.contains_key(*k)).collect();
            extra.sort();
            extra
                .first()
                .map(|k| format!("{path}.{k}: only the snapshot has it"))
        }
        (Value::Array(a), Value::Array(b)) => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                // Entries with an id are named by it, since a position says little.
                let at = match x.get("id").or_else(|| y.get("id")) {
                    Some(id) => format!("{path}[id {id}]"),
                    None => format!("{path}[{i}]"),
                };
                if x.get("id") != y.get("id") {
                    return Some(format!(
                        "{path}[{i}]: the replay has {}, the snapshot has {}",
                        brief(x),
                        brief(y)
                    ));
                }
                if let Some(found) = difference(&at, x, y) {
                    return Some(found);
                }
            }
            Some(format!(
                "{path}: the replay has {} entries and the snapshot has {}",
                a.len(),
                b.len()
            ))
        }
        _ => Some(format!(
            "{path}: the replay has {}, the snapshot has {}",
            brief(replay),
            brief(snapshot)
        )),
    }
}

fn brief(v: &Value) -> String {
    let text = v.to_string();
    if text.chars().count() > 80 {
        let cut: String = text.chars().take(77).collect();
        format!("{cut}...")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(byte: u8) -> Digest {
        Digest::from_bytes([byte; 32])
    }

    fn sample() -> StateImage {
        StateImage {
            last_seq: 9,
            last_at: Some(4_000),
            tasks: vec![TaskImage {
                id: TaskId::from_seq(1),
                created_at: 1_000,
                request: d(1),
                completed_at: None,
                open_holds: vec![HoldId::from_seq(5)],
                cancelled: None,
            }],
            jobs: vec![JobImage {
                id: JobId::from_seq(2),
                task: TaskId::from_seq(1),
                started_at: 2_000,
                ended: Some(JobEndImage {
                    at: 3_000,
                    outcome: JobOutcome::Held,
                    reason: ReasonCode::unavailable(),
                }),
                values: Some(vec![TypedValue::Bool(true)]),
            }],
            holds: vec![HoldImage {
                id: HoldId::from_seq(5),
                task: TaskId::from_seq(1),
                gate: Gate::Egress,
                inputs: d(3),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible(),
                opened_at: 3_000,
                resolved: None,
            }],
            pruned: vec![PrunedImage {
                seq: 11,
                at: 4_500,
                last_seq: 8,
                last_hash: d(9),
            }],
            policy: Some(PolicyImage {
                seq: 12,
                at: 4_600,
                by: Resolution::Human,
                reason: ReasonCode::unavailable(),
                document: "permit (principal, action, resource);".into(),
            }),
            handles: vec![HandleImage {
                seq: 13,
                at: 4_700,
                task: TaskId::from_seq(1),
                handle: d(4),
                by: Resolution::Human,
                reason: ReasonCode::unavailable(),
            }],
            agents: vec![AgentImage {
                id: AgentId::from_seq(14),
                at: 4_800,
                name: AgentName::new("claude-code").unwrap(),
                credential: d(6),
                by: Resolution::Human,
                reason: ReasonCode::unavailable(),
                revoked: Some(RevokedImage {
                    at: 4_900,
                    by: Resolution::Human,
                    reason: ReasonCode::unavailable(),
                }),
            }],
            last_approval: Some(3),
            signed_since: Some(2),
            merges: vec![],
            plugins: vec![],
            install_policy: None,
            pending_updates: vec![],
            trusted_root: None,
            revocations: None,
            revoked_plugins: Vec::new(),
            drafts: Vec::new(),
        }
    }

    #[test]
    fn an_image_survives_encoding() {
        let image = sample();
        let bytes = image.encode().unwrap();
        assert_eq!(StateImage::decode(STATE_VERSION, &bytes).unwrap(), image);
        assert_eq!(image.encode().unwrap(), bytes, "one state, one encoding");
    }

    #[test]
    fn a_version_this_build_does_not_read_is_refused_not_skipped() {
        let bytes = sample().encode().unwrap();
        for version in [0, STATE_VERSION + 1, u32::MAX] {
            assert_eq!(
                StateImage::decode(version, &bytes),
                Err(ImageError::UnsupportedVersion {
                    found: version,
                    current: STATE_VERSION
                })
            );
        }
        let text = ImageError::UnsupportedVersion {
            found: 9,
            current: 1,
        }
        .to_string();
        assert!(text.contains("Upgrade"), "{text}");
    }

    #[test]
    fn a_field_only_one_side_has_is_named_alphabetically_whatever_order_the_map_keeps() {
        // Built key by key in an order that is not alphabetical, as a map that keeps insertion order
        // (serde_json's `preserve_order`, turned on by some dependency) would walk them.
        let object = |keys: &[&str]| {
            let mut map = serde_json::Map::new();
            for key in keys {
                map.insert((*key).to_string(), Value::Null);
            }
            Value::Object(map)
        };
        assert_eq!(
            difference("state", &object(&["b", "a"]), &object(&["b"])).as_deref(),
            Some("state.a: only the replay has it")
        );
        assert_eq!(
            difference("state", &object(&["b"]), &object(&["z", "c"])).as_deref(),
            Some("state.b: only the replay has it")
        );
        assert_eq!(
            difference("state", &object(&["b"]), &object(&["b", "z", "c"])).as_deref(),
            Some("state.c: only the snapshot has it")
        );
    }

    #[test]
    fn the_first_difference_names_the_field_and_both_values() {
        let a = sample();
        assert_eq!(a.first_difference(&a.clone()), None);

        let mut b = a.clone();
        b.last_at = Some(5_000);
        assert_eq!(
            a.first_difference(&b).as_deref(),
            Some("state.last_at: the replay has 4000, the snapshot has 5000")
        );

        // A nested field is named by the entry's id, not its position.
        let mut b = a.clone();
        b.tasks[0].completed_at = Some(9);
        assert_eq!(
            a.first_difference(&b).as_deref(),
            Some("state.tasks[id 1].completed_at: the replay has null, the snapshot has 9")
        );

        // The alphabetically earlier field wins when two differ, whichever way round they are given.
        let mut b = a.clone();
        b.holds[0].opened_at = 1;
        b.last_seq = 10;
        for found in [a.first_difference(&b), b.first_difference(&a)] {
            let found = found.unwrap();
            assert!(found.starts_with("state.holds[id 5].opened_at:"), "{found}");
        }

        // A missing or extra entry is said so.
        let mut b = a.clone();
        b.jobs.clear();
        assert_eq!(
            a.first_difference(&b).as_deref(),
            Some("state.jobs: the replay has 1 entries and the snapshot has 0")
        );
        let mut b = a.clone();
        b.tasks[0].id = TaskId::from_seq(2);
        let found = a.first_difference(&b).unwrap();
        assert!(
            found.starts_with("state.tasks[0]: the replay has"),
            "{found}"
        );
        // And it is symmetric about whether there is a difference.
        assert!(b.first_difference(&a).is_some());
    }

    #[test]
    fn unknown_fields_and_malformed_bytes_are_refused() {
        let mut value: Value = serde_json::from_slice(&sample().encode().unwrap()).unwrap();
        value["surprise"] = true.into();
        let with_extra = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            StateImage::decode(STATE_VERSION, &with_extra),
            Err(ImageError::Decode(_))
        ));
        assert!(StateImage::decode(STATE_VERSION, b"not json").is_err());
        assert!(StateImage::decode(STATE_VERSION, b"{}").is_err());
    }

    use crate::Value;
}
