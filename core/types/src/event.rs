//! The events that tasks, jobs and holds are rebuilt from (ADR-0003, ADR-0009, ADR-0019).
//!
//! Events hold typed values, digests and references, never credentials, tokens or raw text. Each one is
//! stored as JSON with a `kind` tag that repeats the log's `kind` column. Decoding refuses unknown
//! fields, an unknown kind and an unknown schema version, so a payload the engine does not understand
//! is an error and never a guess.

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, AgentName, Answer, Attestation, CommitSha, Digest, Grant, HoldId, IndexLocation,
    InstallPolicy, JobId, MergeMethod, Origin, PluginName, PluginSource, ReasonCode, Schema,
    Signed, Stakes, SubstrateName, TaskId, Timestamp, TypedValue, Upcast, Value, Writers,
};

/// The version these events are written with. Older versions are read by upcasting (ADR-0023).
pub const SCHEMA_VERSION: u32 = 26;

/// What this build reads: version 26, and versions 1 to 25 upcast to it (ADR-0023, ADR-0025, ADR-0027,
/// ADR-0029, ADR-0034, ADR-0043, ADR-0050, ADR-0053, ADR-0057, ADR-0063, ADR-0068, ADR-0069,
/// ADR-0070, ADR-0071, ADR-0089, ADR-0106, ADR-0127).
///
/// Version 2 added the stakes of a hold and the reason for resolving one. A version 1 hold is given the
/// most cautious stakes, so only a human can release it; a version 1 resolution is given an explicit
/// "unrecorded" reason. Version 3 added `archive.pruned` and changed no payload. Every other kind is the
/// same in all versions. Version 4 added `task.cancelled` and `hold.refused` and changed no payload.
/// Version 5 added `policy.approved` and changed no payload. Version 6 added the substrate to
/// `job.started`; an older one reads as `unrecorded`. Version 7 added the credential tier to
/// `job.started`; an older one reads as `unrecorded` too. Version 8 added the reason to `job.ended`;
/// an older one reads as `legacy.unrecorded`. Version 9 added the attestation to `job.started`; an
/// older one reads as `unrecorded`. Version 10 added `handle.released` and changed no payload. Version 11
/// added `agent.issued` and `agent.revoked` and changed no payload. Version 12 added `job.values` and
/// changed no payload. Version 13 added `signed` to every decision (`hold.resolved`, `task.cancelled`,
/// `policy.approved`, `handle.released`, `agent.issued`, `agent.revoked`): an older human decision
/// reads as `unrecorded`, any other as `none`. Version 14 added `merge.approved` and changed no
/// payload. Version 15 added `plugin.installed` and changed no payload. Version 16 added a plugin
/// index as a `plugin.installed` source (ADR-0090); a payload written before reads as it was. Version
/// 17 added `plugin.policy_set` and changed no payload. Version 18 added `plugin.update_pending` and
/// changed no payload. Version 19 added `plugin.snapshot_imported` (ADR-0097) and changed no payload.
/// Version 20 added `plugin.revocations_imported` and `plugin.revoked` (ADR-0098) and changed no
/// payload. Version 21 added `writers` to `handle.released` (ADR-0103); a payload written before has
/// none and reads as it was. Version 22 added the `dependency` gate (ADR-0106), which `gate.answered`
/// and `hold.opened` may name, and changed no payload. Version 23 added `skill.drafted` (ADR-0113)
/// and changed no payload. Version 24 added `skill.trusted` (ADR-0114) and changed no payload.
/// Version 25 added `signed_at` to an index plugin's source in `plugin.installed` (ADR-0127); an
/// earlier event has none and reads as it was. Version 26 added `plugin.index_accepted` (ADR-0128)
/// and changed no payload.
pub static SCHEMA: Schema = Schema {
    current: SCHEMA_VERSION,
    steps: &[
        Upcast::new("hold.opened", 1, hold_opened_v1),
        Upcast::new("hold.resolved", 1, hold_resolved_v1),
        Upcast::new("job.started", 5, job_started_v5),
        Upcast::new("job.started", 6, job_started_v6),
        Upcast::new("job.ended", 7, job_ended_v7),
        Upcast::new("job.started", 8, job_started_v8),
        Upcast::new("hold.resolved", 12, decision_v12),
        Upcast::new("task.cancelled", 12, decision_v12),
        Upcast::new("policy.approved", 12, decision_v12),
        Upcast::new("handle.released", 12, decision_v12),
        Upcast::new("agent.issued", 12, decision_v12),
        Upcast::new("agent.revoked", 12, decision_v12),
    ],
};

fn insert(mut payload: Value, field: &str, value: Value) -> Result<Value, String> {
    payload
        .as_object_mut()
        .ok_or("the payload is not an object")?
        .insert(field.to_string(), value);
    Ok(payload)
}

fn hold_opened_v1(payload: Value) -> Result<Value, String> {
    insert(
        payload,
        "stakes",
        serde_json::json!({ "tier": "irreversible", "touches": [] }),
    )
}

fn hold_resolved_v1(payload: Value) -> Result<Value, String> {
    insert(payload, "reason", Value::String("legacy.unrecorded".into()))
}

fn job_started_v5(payload: Value) -> Result<Value, String> {
    insert(payload, "substrate", Value::String("unrecorded".into()))
}

fn job_started_v6(payload: Value) -> Result<Value, String> {
    insert(payload, "credentials", Value::String("unrecorded".into()))
}

fn job_started_v8(payload: Value) -> Result<Value, String> {
    insert(payload, "attestation", Value::String("unrecorded".into()))
}

/// Before version 13 no decision was signed: a human's reads as `unrecorded`, any other as `none`.
fn decision_v12(payload: Value) -> Result<Value, String> {
    let human = payload.get("by").and_then(Value::as_str) == Some("human");
    let signed = if human { "unrecorded" } else { "none" };
    insert(payload, "signed", Value::String(signed.into()))
}

fn job_ended_v7(payload: Value) -> Result<Value, String> {
    insert(payload, "reason", Value::String("legacy.unrecorded".into()))
}

/// Where a job's credentials are held, which bounds what a compromised sandbox could take
/// (ADR-0016, ADR-0050). In v1 every key and token is held by the broker and none enters the sandbox.
/// A credential inside the sandbox would be its own, weaker tier, recorded as such if it is ever built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialTier {
    /// The broker holds the provider key and the forge token and adds them to requests itself.
    BrokerHeld,
    /// A `job.started` written before schema version 7, which did not say.
    Unrecorded,
}

/// How a job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOutcome {
    Succeeded,
    Failed,
    /// The manifest's deadline passed (ADR-0004).
    DeadlineExceeded,
    /// The job was stopped because its task is being held (ADR-0009).
    Held,
}

/// The gates that answer per request (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    Egress,
    Content,
    Role,
    Manifest,
    /// Whether what the job runs on is still there: the broker, the sandbox, the model provider and
    /// the forge. A dependency lost mid-job answers unknown, so the task holds (ADR-0106).
    Dependency,
}

/// Why a hold was opened. A gate that answers `deny` does not hold; it refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// The gate answered unknown.
    Unknown,
    /// The gate did not answer in time, which counts as unknown.
    TimedOut,
}

/// How a hold was resolved: the tiers of ADR-0012, or the gate answering on re-evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Reevaluated,
    Policy,
    Verifier,
    Human,
}

/// One state change. Identifiers of new things are not in the event: they are the sequence number it
/// is appended at ([`crate::ids`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Event {
    /// A task was asked for. `request` is the digest of the request document, which is held elsewhere.
    #[serde(rename = "task.created")]
    TaskCreated { request: Digest },
    #[serde(rename = "task.completed")]
    TaskCompleted { task: TaskId },
    /// A job started on `task`, in a sandbox from the substrate named, with its credentials held
    /// where `credentials` says and what the substrate could attest to (ADR-0011, ADR-0043,
    /// ADR-0050, ADR-0057).
    #[serde(rename = "job.started")]
    JobStarted {
        task: TaskId,
        substrate: SubstrateName,
        credentials: CredentialTier,
        attestation: Attestation,
    },
    /// A job ended, how, and why in a short code: the harness finished, the deadline passed, the
    /// broker refused, the operator interrupted the run (ADR-0053).
    #[serde(rename = "job.ended")]
    JobEnded {
        job: JobId,
        outcome: JobOutcome,
        reason: ReasonCode,
    },
    /// A gate answered a request made by a running job. `inputs` is the digest of what it was asked.
    /// An `unknown` answer is always followed, in the same write, by the job ending held and the hold
    /// opening (ADR-0020).
    #[serde(rename = "gate.answered")]
    GateAnswered {
        job: JobId,
        gate: Gate,
        inputs: Digest,
        answer: Answer,
        reason: ReasonCode,
    },
    /// A gate answered unknown or timed out. `inputs` is the digest of what the gate was asked, and
    /// `stakes` says how much the held action matters, which decides who may release it (ADR-0025).
    #[serde(rename = "hold.opened")]
    HoldOpened {
        task: TaskId,
        gate: Gate,
        inputs: Digest,
        reason: HoldReason,
        stakes: Stakes,
    },
    /// A hold was released, by whom, and the short code for why. The state refuses a release by anyone
    /// not allowed at the hold's stakes (ADR-0025).
    #[serde(rename = "hold.resolved")]
    HoldResolved {
        hold: HoldId,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// An archive of compacted events was deleted from the machine (ADR-0026). It names the last event
    /// the archive held and that event's hash, which is also where the snapshot that stands for the
    /// archive stands, so the record cannot be about some other log's archive. The engine writes it
    /// before the file is removed.
    #[serde(rename = "archive.pruned")]
    ArchivePruned { last_seq: u64, last_hash: Digest },
    /// A held task was ended by a decision, so it has no open holds and nothing can start on it. `by`
    /// is the tier of decider (never a gate re-evaluation) and the state refuses a decider the stakes
    /// of every open hold do not allow (ADR-0029).
    #[serde(rename = "task.cancelled")]
    TaskCancelled {
        task: TaskId,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// A held request was put to its gate again and the gate refused. This is evidence that the action
    /// is refused now. It releases nothing and ends nothing (ADR-0029).
    #[serde(rename = "hold.refused")]
    HoldRefused { hold: HoldId, reason: ReasonCode },
    /// A human approved this Cedar document as the operator's policy, and from this event on it is the
    /// policy in force (ADR-0003, ADR-0034). It is the document itself, not a digest: the policy in
    /// force is the projection of the log, so the log must hold it. It is operator-written policy
    /// text, which carries no secret. `by` must be a human, since a policy change is irreversible and
    /// touches policy (ADR-0012, ADR-0025), and the state refuses anyone else.
    #[serde(rename = "policy.approved")]
    PolicyApproved {
        document: String,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// The raw text the broker holds as `handle` was released to the task's actor (ADR-0005,
    /// ADR-0063). The handle is the SHA-256 of the bytes the broker holds, so the decision is bound to
    /// exactly that text: the engine never sees it, and the broker serves only bytes with this digest.
    /// A human may release any; policy only text whose `writers` all can write to the repository
    /// (ADR-0025's routine stakes, ADR-0103), and the state refuses anyone else.
    #[serde(rename = "handle.released")]
    HandleReleased {
        task: TaskId,
        handle: Digest,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
        /// Who wrote the text, as the broker read it with the hold. A human's release has none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        writers: Option<Writers>,
    },
    /// A human issued a credential to a driving agent, which the engine's MCP server admits from this
    /// event on (ADR-0010, ADR-0068). `credential` is the SHA-256 of the secret the agent presents;
    /// the secret is never in the log. Issuing one grants a capability, so it needs a human (ADR-0012),
    /// and the state refuses anyone else. Its id is this event's sequence number.
    #[serde(rename = "agent.issued")]
    AgentIssued {
        name: AgentName,
        credential: Digest,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// A credential was revoked: the MCP server refuses it from this event on (ADR-0068). Revoking
    /// only takes a capability away, so any decider may.
    #[serde(rename = "agent.revoked")]
    AgentRevoked {
        agent: AgentId,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// A human approved merging pull request `pr` of `repo` at commit `head`, by `method`, as task
    /// `task`'s one merge (ADR-0012, ADR-0071). The approval names the commit the operator reviewed:
    /// the broker merges only if the pull request's head is still that commit, so a push after it
    /// voids it. A merge is irreversible, so the state refuses anyone but a human, and a task holds at
    /// most one. `repo` is an `owner/name` identifier, not forge text.
    #[serde(rename = "merge.approved")]
    MergeApproved {
        task: TaskId,
        repo: String,
        pr: u64,
        head: CommitSha,
        method: MergeMethod,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// A human installed plugin `name`, whose files have SHA-256 `digest` in the plugin store, and
    /// granted it `grant` (ADR-0006, ADR-0089). The files are never in the log: the digest binds the
    /// decision to exactly those bytes, and they are checked against it each time they are used. A
    /// later install of the same name replaces this one. Granting a plugin capabilities is an
    /// irreversible capability grant, so the state refuses anyone but a human, and a grant with a name
    /// the vocabulary does not accept (ADR-0085).
    #[serde(rename = "plugin.installed")]
    PluginInstalled {
        name: PluginName,
        digest: Digest,
        grant: Grant,
        source: PluginSource,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// The operator set the install policy (ADR-0007, ADR-0091): a plugin from one of its indexes that
    /// asks for nothing beyond its ceiling may then be installed by policy. It replaces any earlier
    /// one. Setting it decides what installs without a human, so only a human sets it.
    #[serde(rename = "plugin.policy_set")]
    PluginPolicySet {
        policy: InstallPolicy,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
    },
    /// An update check found `version` of plugin `name` in the index at `location`, with digest
    /// `digest`, asking for `asks`, beyond what the install policy lets install without the operator
    /// (ADR-0092). It is not installed: the installed version keeps running until a human installs
    /// this one, or a later check finds it covered. It is not a decision, and an install of the name
    /// clears it.
    #[serde(rename = "plugin.update_pending")]
    PluginUpdatePending {
        name: PluginName,
        version: crate::PluginVersion,
        digest: Digest,
        asks: Grant,
        location: IndexLocation,
    },
    /// A trusted-root snapshot was imported (ADR-0097): the file with digest `digest`, which the
    /// snapshot in force vouched for, issued at `signed_at` by the earliest signed time its bundle
    /// proves. From then on releases are verified against it. It is not a decision; the engine refuses
    /// one no later than the last.
    #[serde(rename = "plugin.snapshot_imported")]
    PluginSnapshotImported {
        digest: Digest,
        signed_at: Timestamp,
    },
    /// A revocation list was imported (ADR-0098): the file with digest `digest`, signed by the
    /// marketplace pipeline and verified against the trusted-root snapshot in force, issued at
    /// `signed_at`. Installs are checked against it from then on. It is not a decision; the engine
    /// refuses one no later than the last.
    #[serde(rename = "plugin.revocations_imported")]
    PluginRevocationsImported {
        digest: Digest,
        signed_at: Timestamp,
    },
    /// The signed index at `location` was accepted (ADR-0128): the file with digest `digest`, signed
    /// by the marketplace pipeline and verified against the trusted-root snapshot in force, issued at
    /// `signed_at`. An update from that location is refused an older index from then on. It is not a
    /// decision; the engine refuses one no later than the last accepted there.
    #[serde(rename = "plugin.index_accepted")]
    PluginIndexAccepted {
        location: IndexLocation,
        digest: Digest,
        signed_at: Timestamp,
    },
    /// The installed release of plugin `name`, with digest `digest`, is revoked for `reason`, as the
    /// revocation list in force says (ADR-0098). It is disabled: no job is given it until a release
    /// that is not revoked is installed under the name. Not an operator's decision: the maintainers
    /// revoked it, and the engine records that it applies here.
    #[serde(rename = "plugin.revoked")]
    PluginRevoked {
        name: PluginName,
        digest: Digest,
        reason: ReasonCode,
    },
    /// The typed values a running job returned: a reader's or planner's answer, or an actor's submitted
    /// result (ADR-0062, ADR-0064, ADR-0069). A choice is its option's index, so no text is stored. It
    /// is written together with the job's ending, so a resumed run finds every finished step's values
    /// in the log.
    #[serde(rename = "job.values")]
    JobValues { job: JobId, values: Vec<TypedValue> },
    /// Hyalos drafted skill `name` from task `task`, a run that succeeded (ADR-0113). Its files, with
    /// SHA-256 `digest`, are in the plugin store. Hyalos wrote them from the run's typed values and
    /// the workflow's own words, never from text the forge or a model wrote; but a model chose those
    /// values after reading the issue, so the draft is tainted and `grant` holds no more than
    /// `Grant::read_only`, which the state refuses otherwise. It is not a decision: it grants nothing
    /// a job did not already have.
    #[serde(rename = "skill.drafted")]
    SkillDrafted {
        name: PluginName,
        digest: Digest,
        task: TaskId,
        grant: Grant,
    },
    /// Draft `name`, with SHA-256 `digest`, was replayed from the signed log and passed its
    /// capability checks, so it is trusted (ADR-0114): its text counts as `PluginAuthored`, and a
    /// workflow may name it. Its grant stays the read-only one it was drafted with. A draft that
    /// fails replay never gets this event and stays tainted.
    #[serde(rename = "skill.trusted")]
    SkillTrusted { name: PluginName, digest: Digest },
}

/// The largest policy document an event may hold, in bytes. Events are read whole into memory by
/// replay, so a document is bounded.
pub const MAX_POLICY_DOCUMENT: usize = 256 * 1024;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    #[error("cannot encode the event: {0}")]
    Encode(String),
    #[error("cannot decode the event: {0}")]
    Decode(String),
    /// The row's schema version is 0 or newer than this build reads (ADR-0023).
    #[error("{}", unsupported_version(.found, .current))]
    UnsupportedVersion { found: u32, current: u32 },
    /// An upcast step refused the payload it was given.
    #[error("cannot bring a `{kind}` event written at schema version {from} up to date: {reason}")]
    Upcast {
        kind: String,
        from: u32,
        reason: String,
    },
    /// The log's `kind` column and the payload's own `kind` disagree.
    #[error("the log says this is a `{column}` event but its payload is a `{payload}` event")]
    KindMismatch {
        column: String,
        payload: &'static str,
    },
}

fn unsupported_version(found: &u32, current: &u32) -> String {
    match found {
        0 => "event schema version 0 is not valid; versions start at 1".to_owned(),
        _ => format!(
            "event schema version {found} was written by a newer Hyalos; this build reads up to version {current}. Upgrade Hyalos to continue this log (there is no downgrade)"
        ),
    }
}

/// The log's `kind` value of every event, in one table so [`Event::kind`] and [`Event::KINDS`] agree.
macro_rules! kinds {
    ($($variant:ident => $kind:literal,)*) => {
        impl Event {
            /// The value for the log's `kind` column.
            pub fn kind(&self) -> &'static str {
                match self {
                    $(Event::$variant { .. } => $kind,)*
                }
            }

            /// Every `kind` value, for checking tables and test coverage.
            pub const KINDS: [&'static str; 25] = [$($kind,)*];
        }
    };
}

kinds! {
    TaskCreated => "task.created",
    TaskCompleted => "task.completed",
    JobStarted => "job.started",
    JobEnded => "job.ended",
    GateAnswered => "gate.answered",
    HoldOpened => "hold.opened",
    HoldResolved => "hold.resolved",
    ArchivePruned => "archive.pruned",
    TaskCancelled => "task.cancelled",
    HoldRefused => "hold.refused",
    PolicyApproved => "policy.approved",
    HandleReleased => "handle.released",
    AgentIssued => "agent.issued",
    AgentRevoked => "agent.revoked",
    MergeApproved => "merge.approved",
    PluginInstalled => "plugin.installed",
    PluginPolicySet => "plugin.policy_set",
    PluginUpdatePending => "plugin.update_pending",
    PluginSnapshotImported => "plugin.snapshot_imported",
    PluginRevocationsImported => "plugin.revocations_imported",
    PluginIndexAccepted => "plugin.index_accepted",
    PluginRevoked => "plugin.revoked",
    JobValues => "job.values",
    SkillDrafted => "skill.drafted",
    SkillTrusted => "skill.trusted",
}

impl Event {
    /// Whether `origin` may submit this event (ADR-0024). The broker may report only what a gate
    /// answered, since it evaluates gates per request (ADR-0009). Everything else is the engine's: it
    /// creates and completes tasks, starts and ends jobs, and opens and resolves holds, so a broker
    /// that is compromised cannot release a hold it caused or finish work it did not run.
    pub fn may_come_from(&self, origin: Origin) -> bool {
        match self {
            Event::GateAnswered { .. } => true,
            Event::TaskCreated { .. }
            | Event::TaskCompleted { .. }
            | Event::JobStarted { .. }
            | Event::JobEnded { .. }
            | Event::HoldOpened { .. }
            | Event::HoldResolved { .. }
            | Event::ArchivePruned { .. }
            | Event::TaskCancelled { .. }
            | Event::HoldRefused { .. }
            | Event::PolicyApproved { .. }
            | Event::HandleReleased { .. }
            | Event::AgentIssued { .. }
            | Event::AgentRevoked { .. }
            | Event::MergeApproved { .. }
            | Event::PluginInstalled { .. }
            | Event::PluginPolicySet { .. }
            | Event::PluginUpdatePending { .. }
            | Event::PluginSnapshotImported { .. }
            | Event::PluginRevocationsImported { .. }
            | Event::PluginIndexAccepted { .. }
            | Event::PluginRevoked { .. }
            | Event::JobValues { .. }
            | Event::SkillDrafted { .. }
            | Event::SkillTrusted { .. } => origin == Origin::Engine,
        }
    }

    /// Who decided, for the nine kinds that record a decision (ADR-0070, ADR-0071, ADR-0089, ADR-0091).
    pub fn decided_by(&self) -> Option<Resolution> {
        match self {
            Event::HoldResolved { by, .. }
            | Event::TaskCancelled { by, .. }
            | Event::PolicyApproved { by, .. }
            | Event::HandleReleased { by, .. }
            | Event::AgentIssued { by, .. }
            | Event::AgentRevoked { by, .. }
            | Event::MergeApproved { by, .. }
            | Event::PluginInstalled { by, .. }
            | Event::PluginPolicySet { by, .. } => Some(*by),
            _ => None,
        }
    }

    /// The signature a decision carries, for the same nine kinds.
    pub fn signed(&self) -> Option<&Signed> {
        match self {
            Event::HoldResolved { signed, .. }
            | Event::TaskCancelled { signed, .. }
            | Event::PolicyApproved { signed, .. }
            | Event::HandleReleased { signed, .. }
            | Event::AgentIssued { signed, .. }
            | Event::AgentRevoked { signed, .. }
            | Event::MergeApproved { signed, .. }
            | Event::PluginInstalled { signed, .. }
            | Event::PluginPolicySet { signed, .. } => Some(signed),
            _ => None,
        }
    }

    /// The decision with `signed` in place of what it carried. Other kinds are returned as they are.
    pub fn with_signed(mut self, value: Signed) -> Self {
        match &mut self {
            Event::HoldResolved { signed, .. }
            | Event::TaskCancelled { signed, .. }
            | Event::PolicyApproved { signed, .. }
            | Event::HandleReleased { signed, .. }
            | Event::AgentIssued { signed, .. }
            | Event::AgentRevoked { signed, .. }
            | Event::MergeApproved { signed, .. }
            | Event::PluginInstalled { signed, .. }
            | Event::PluginPolicySet { signed, .. } => *signed = value,
            _ => {}
        }
        self
    }

    /// The bytes the operator signs for this decision: the event as written, with [`Signed::None`] in
    /// place of its signature, so the signature covers every other field (ADR-0070).
    pub fn approval_bytes(&self) -> Result<Vec<u8>, EventError> {
        self.clone().with_signed(Signed::None).encode()
    }

    /// The bytes stored in the log's `payload` column.
    pub fn encode(&self) -> Result<Vec<u8>, EventError> {
        serde_json::to_vec(self).map_err(|e| EventError::Encode(e.to_string()))
    }

    /// Reads an event back from a log row's `kind`, `schema_version` and `payload`, as this build's
    /// [`SCHEMA`] understands it.
    pub fn decode(kind: &str, schema_version: u32, payload: &[u8]) -> Result<Self, EventError> {
        Self::decode_with(&SCHEMA, kind, schema_version, payload)
    }

    /// Like [`Event::decode`], under another schema. A row older than `schema.current` is upcast in
    /// memory first (ADR-0023); a row at the current version is read as stored.
    pub fn decode_with(
        schema: &Schema,
        kind: &str,
        schema_version: u32,
        payload: &[u8],
    ) -> Result<Self, EventError> {
        schema.check_version(schema_version)?;
        let event: Event = if schema_version == schema.current {
            serde_json::from_slice(payload)
        } else {
            let value =
                serde_json::from_slice(payload).map_err(|e| EventError::Decode(e.to_string()))?;
            serde_json::from_value(schema.upcast(kind, schema_version, value)?)
        }
        .map_err(|e| EventError::Decode(e.to_string()))?;
        if event.kind() != kind {
            return Err(EventError::KindMismatch {
                column: kind.to_string(),
                payload: event.kind(),
            });
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sensitive;

    fn d(byte: u8) -> Digest {
        Digest::from_bytes([byte; 32])
    }

    /// The operator key from seed 7 and its signature in the pinned `hold.resolved`. Ed25519 signing
    /// is deterministic, so these never change.
    const OPERATOR: &str = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
    const SIGNATURE: &str = "9917aead5135233d25540b04e458996ebe7f1f41fc3d3e3ebd241f335b414080334096d65f4ab7729f316332350aba267c69ce6233ddf1569b5698d90150bf0f";

    fn samples() -> Vec<(Event, String)> {
        let h = |b: u8| format!("{b:02x}").repeat(32);
        vec![
            (
                Event::TaskCreated { request: d(1) },
                format!(r#"{{"kind":"task.created","request":"{}"}}"#, h(1)),
            ),
            (
                Event::TaskCompleted {
                    task: TaskId::from_seq(3),
                },
                r#"{"kind":"task.completed","task":3}"#.to_string(),
            ),
            (
                Event::JobStarted {
                    task: TaskId::from_seq(3),
                    substrate: SubstrateName::new("container.podman").unwrap(),
                    credentials: crate::CredentialTier::BrokerHeld,
                    attestation: crate::Attestation::no_hardware_root(),
                },
                r#"{"kind":"job.started","task":3,"substrate":"container.podman","credentials":"broker_held","attestation":{"none":"attestation.no_hardware_root"}}"#
                    .to_string(),
            ),
            (
                Event::JobEnded {
                    job: JobId::from_seq(4),
                    outcome: JobOutcome::DeadlineExceeded,
                    reason: ReasonCode::new("job.deadline").unwrap(),
                },
                r#"{"kind":"job.ended","job":4,"outcome":"deadline_exceeded","reason":"job.deadline"}"#
                    .to_string(),
            ),
            (
                Event::GateAnswered {
                    job: JobId::from_seq(4),
                    gate: Gate::Content,
                    inputs: d(6),
                    answer: Answer::Deny,
                    reason: ReasonCode::new("content.secret_pattern").unwrap(),
                },
                format!(
                    r#"{{"kind":"gate.answered","job":4,"gate":"content","inputs":"{}","answer":"deny","reason":"content.secret_pattern"}}"#,
                    h(6)
                ),
            ),
            (
                Event::HoldOpened {
                    task: TaskId::from_seq(3),
                    gate: Gate::Egress,
                    inputs: d(2),
                    reason: HoldReason::TimedOut,
                    stakes: Stakes::escalated()
                        .touching(Sensitive::CapabilityGrant)
                        .touching(Sensitive::Credentials),
                },
                format!(
                    r#"{{"kind":"hold.opened","task":3,"gate":"egress","inputs":"{}","reason":"timed_out","stakes":{{"tier":"escalated","touches":["credentials","capability_grant"]}}}}"#,
                    h(2)
                ),
            ),
            (
                Event::HoldResolved {
                    hold: HoldId::from_seq(5),
                    by: Resolution::Human,
                    reason: ReasonCode::new("approved.operator").unwrap(),
                    signed: Signed::Operator(crate::OperatorKey::from_seed(&[7; 32]).approve(
                        &[1; 32],
                        4,
                        d(2),
                        b"the event",
                    )),
                },
                format!(
                    r#"{{"kind":"hold.resolved","hold":5,"by":"human","reason":"approved.operator","signed":{{"operator":{{"operator":"{OPERATOR}","after":4,"after_hash":"{}","signature":"{SIGNATURE}"}}}}}}"#,
                    h(2)
                ),
            ),
            (
                Event::ArchivePruned {
                    last_seq: 42,
                    last_hash: d(7),
                },
                format!(
                    r#"{{"kind":"archive.pruned","last_seq":42,"last_hash":"{}"}}"#,
                    h(7)
                ),
            ),
            (
                Event::TaskCancelled {
                    task: TaskId::from_seq(3),
                    by: Resolution::Human,
                    reason: ReasonCode::new("cancelled.operator").unwrap(),
                    signed: Signed::Unrecorded,
                },
                r#"{"kind":"task.cancelled","task":3,"by":"human","reason":"cancelled.operator","signed":"unrecorded"}"#
                    .to_string(),
            ),
            (
                Event::HoldRefused {
                    hold: HoldId::from_seq(5),
                    reason: ReasonCode::new("egress.denied").unwrap(),
                },
                r#"{"kind":"hold.refused","hold":5,"reason":"egress.denied"}"#.to_string(),
            ),
            (
                Event::PolicyApproved {
                    document: "permit (principal, action, resource);".into(),
                    by: Resolution::Human,
                    reason: ReasonCode::new("policy.approved").unwrap(),
                    signed: Signed::Unrecorded,
                },
                r#"{"kind":"policy.approved","document":"permit (principal, action, resource);","by":"human","reason":"policy.approved","signed":"unrecorded"}"#
                    .to_string(),
            ),
            (
                Event::HandleReleased {
                    task: TaskId::from_seq(3),
                    handle: d(9),
                    by: Resolution::Human,
                    reason: ReasonCode::new("handle.operator").unwrap(),
                    signed: Signed::Unrecorded,
                    writers: None,
                },
                format!(
                    r#"{{"kind":"handle.released","task":3,"handle":"{}","by":"human","reason":"handle.operator","signed":"unrecorded"}}"#,
                    h(9)
                ),
            ),
            (
                Event::AgentIssued {
                    name: crate::AgentName::new("claude-code").unwrap(),
                    credential: d(5),
                    by: Resolution::Human,
                    reason: ReasonCode::new("agent.issued").unwrap(),
                    signed: Signed::Unrecorded,
                },
                format!(
                    r#"{{"kind":"agent.issued","name":"claude-code","credential":"{}","by":"human","reason":"agent.issued","signed":"unrecorded"}}"#,
                    h(5)
                ),
            ),
            (
                Event::AgentRevoked {
                    agent: crate::AgentId::from_seq(4),
                    by: Resolution::Policy,
                    reason: ReasonCode::new("agent.revoked").unwrap(),
                    signed: Signed::None,
                },
                r#"{"kind":"agent.revoked","agent":4,"by":"policy","reason":"agent.revoked","signed":"none"}"#
                    .to_string(),
            ),
            (
                Event::MergeApproved {
                    task: TaskId::from_seq(1),
                    repo: "octo/repo".into(),
                    pr: 42,
                    head: CommitSha::new("0123456789abcdef0123456789abcdef01234567").unwrap(),
                    method: MergeMethod::Squash,
                    by: Resolution::Human,
                    reason: ReasonCode::new("merge.operator").unwrap(),
                    signed: Signed::Unrecorded,
                },
                r#"{"kind":"merge.approved","task":1,"repo":"octo/repo","pr":42,"head":"0123456789abcdef0123456789abcdef01234567","method":"squash","by":"human","reason":"merge.operator","signed":"unrecorded"}"#
                    .to_string(),
            ),
            (
                Event::PluginInstalled {
                    name: PluginName::new("pdf-tools").unwrap(),
                    digest: d(6),
                    grant: Grant {
                        egress_hosts: ["docs.rs".to_string()].into(),
                        ..Grant::none()
                    },
                    source: PluginSource::Local,
                    by: Resolution::Human,
                    reason: ReasonCode::new("plugin.local_install").unwrap(),
                    signed: Signed::Unrecorded,
                },
                format!(
                    r#"{{"kind":"plugin.installed","name":"pdf-tools","digest":"{}","grant":{{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]}},"source":"local","by":"human","reason":"plugin.local_install","signed":"unrecorded"}}"#,
                    h(6)
                ),
            ),
            (
                Event::PluginPolicySet {
                    policy: InstallPolicy {
                        ceiling: Grant {
                            egress_hosts: ["docs.rs".to_string()].into(),
                            ..Grant::none()
                        },
                        indexes: [crate::IndexLocation::new("https://plugins.example/v1").unwrap()]
                            .into(),
                        max_snapshot_age_days: 90,
                        max_revocation_age_days: 30,
                    },
                    by: Resolution::Human,
                    reason: ReasonCode::new("plugin.policy").unwrap(),
                    signed: Signed::Unrecorded,
                },
                r#"{"kind":"plugin.policy_set","policy":{"ceiling":{"actions":[],"egress_hosts":["docs.rs"],"tools":[],"data_flows":[]},"indexes":["https://plugins.example/v1"],"max_snapshot_age_days":90,"max_revocation_age_days":30},"by":"human","reason":"plugin.policy","signed":"unrecorded"}"#
                    .to_string(),
            ),
            (
                Event::PluginSnapshotImported {
                    digest: d(8),
                    signed_at: Timestamp::from_millis(1_794_878_463_000),
                },
                format!(
                    r#"{{"kind":"plugin.snapshot_imported","digest":"{}","signed_at":1794878463000}}"#,
                    h(8)
                ),
            ),
            (
                Event::PluginRevocationsImported {
                    digest: d(9),
                    signed_at: Timestamp::from_millis(1_794_882_145_000),
                },
                format!(
                    r#"{{"kind":"plugin.revocations_imported","digest":"{}","signed_at":1794882145000}}"#,
                    h(9)
                ),
            ),
            (
                Event::PluginIndexAccepted {
                    location: IndexLocation::new("https://plugins.example/v1").unwrap(),
                    digest: d(9),
                    signed_at: Timestamp::from_millis(1_794_950_000_000),
                },
                format!(
                    r#"{{"kind":"plugin.index_accepted","location":"https://plugins.example/v1","digest":"{}","signed_at":1794950000000}}"#,
                    h(9)
                ),
            ),
            (
                Event::PluginRevoked {
                    name: PluginName::new("pdf-tools").unwrap(),
                    digest: d(10),
                    reason: ReasonCode::new("malware.exfiltration").unwrap(),
                },
                format!(
                    r#"{{"kind":"plugin.revoked","name":"pdf-tools","digest":"{}","reason":"malware.exfiltration"}}"#,
                    h(10)
                ),
            ),
            (
                Event::PluginUpdatePending {
                    name: PluginName::new("pdf-tools").unwrap(),
                    version: crate::PluginVersion::new("2.0.0").unwrap(),
                    digest: d(7),
                    asks: Grant {
                        actions: ["push".to_string()].into(),
                        ..Grant::none()
                    },
                    location: IndexLocation::new("https://plugins.example/v1").unwrap(),
                },
                format!(
                    r#"{{"kind":"plugin.update_pending","name":"pdf-tools","version":"2.0.0","digest":"{}","asks":{{"actions":["push"],"egress_hosts":[],"tools":[],"data_flows":[]}},"location":"https://plugins.example/v1"}}"#,
                    h(7)
                ),
            ),
            (
                Event::JobValues {
                    job: JobId::from_seq(4),
                    values: vec![
                        TypedValue::Choice(2),
                        TypedValue::Bool(true),
                        TypedValue::Integer(-3),
                    ],
                },
                r#"{"kind":"job.values","job":4,"values":[{"choice":2},{"bool":true},{"integer":-3}]}"#
                    .to_string(),
            ),
            (
                Event::SkillDrafted {
                    name: PluginName::new("draft-3").unwrap(),
                    digest: d(9),
                    task: TaskId::from_seq(3),
                    grant: Grant::read_only(),
                },
                format!(
                    r#"{{"kind":"skill.drafted","name":"draft-3","digest":"{}","task":3,"grant":{{"actions":["read"],"egress_hosts":[],"tools":[],"data_flows":["job->model"]}}}}"#,
                    h(9)
                ),
            ),
            (
                Event::SkillTrusted {
                    name: PluginName::new("draft-3").unwrap(),
                    digest: d(9),
                },
                format!(
                    r#"{{"kind":"skill.trusted","name":"draft-3","digest":"{}"}}"#,
                    h(9)
                ),
            ),
        ]
    }

    #[test]
    fn every_event_has_a_pinned_encoding_and_round_trips() {
        // A change to these strings changes what is stored in every log, so it must be deliberate.
        for (event, json) in samples() {
            assert_eq!(String::from_utf8(event.encode().unwrap()).unwrap(), json);
            assert_eq!(
                Event::decode(event.kind(), SCHEMA_VERSION, json.as_bytes()).unwrap(),
                event
            );
        }
    }

    #[test]
    fn only_a_gate_answer_may_come_from_the_broker() {
        // The matrix of ADR-0024, one row per kind.
        let samples = samples();
        assert_eq!(samples.len(), Event::KINDS.len());
        for (event, _) in samples {
            assert!(event.may_come_from(Origin::Engine), "{}", event.kind());
            assert_eq!(
                event.may_come_from(Origin::Broker),
                event.kind() == "gate.answered",
                "{}",
                event.kind()
            );
        }
    }

    #[test]
    fn the_kind_column_matches_the_payload_tag() {
        for (event, json) in samples() {
            assert!(json.contains(&format!("\"kind\":\"{}\"", event.kind())));
        }
    }

    #[test]
    fn a_wrong_kind_column_is_refused() {
        let json = r#"{"kind":"job.started","task":3,"substrate":"container.docker","credentials":"broker_held","attestation":"unrecorded"}"#;
        let err = Event::decode("task.completed", SCHEMA_VERSION, json.as_bytes()).unwrap_err();
        assert!(matches!(err, EventError::KindMismatch { .. }), "{err}");
    }

    #[test]
    fn a_malformed_gate_answer_is_a_decode_error_not_just_a_kind_mismatch() {
        let zeros = "00".repeat(32);
        let good = format!(
            r#"{{"kind":"gate.answered","job":4,"gate":"egress","inputs":"{zeros}","answer":"allow","reason":"ok"}}"#
        );
        assert!(Event::decode("gate.answered", SCHEMA_VERSION, good.as_bytes()).is_ok());
        let bad = [
            good.replace(&zeros, "00"),
            good.replace("\"ok\"", "\"Free Text Here\""),
            good.replace("\"ok\"", "\"\""),
            good.replace("\"allow\"", "\"perhaps\""),
            good.replace("\"allow\"", "null"),
            good.replace(",\"reason\":\"ok\"", ""),
            good.replace("}", ",\"note\":\"x\"}"),
            good.replace("\"job\":4", "\"job\":\"4\""),
        ];
        for json in bad {
            let err = Event::decode("gate.answered", SCHEMA_VERSION, json.as_bytes()).unwrap_err();
            assert!(matches!(err, EventError::Decode(_)), "{json}: {err}");
        }
    }

    #[test]
    fn unknown_fields_kinds_and_versions_are_refused() {
        let bad = [
            r#"{"kind":"job.started","task":3,"substrate":"local","credentials":"broker_held","attestation":"unrecorded","extra":1}"#,
            // Since version 6 a job names its substrate, since 7 its credential tier and since 9 its
            // attestation; only an older event may leave them out.
            r#"{"kind":"job.started","task":3,"substrate":"local","credentials":"broker_held"}"#,
            r#"{"kind":"job.started","task":3,"substrate":"local","credentials":"broker_held","attestation":{"evidence":"00"}}"#,
            r#"{"kind":"job.started","task":3}"#,
            r#"{"kind":"job.started","task":3,"substrate":"local"}"#,
            r#"{"kind":"job.started","task":3,"substrate":"local","credentials":"in_sandbox"}"#,
            r#"{"kind":"job.started","task":3,"substrate":"/usr/bin/podman"}"#,
            r#"{"kind":"job.vanished","task":3}"#,
            r#"{"kind":"job.started"}"#,
            r#"{"kind":"job.started","task":"3"}"#,
            r#"{"kind":"job.ended","job":4,"outcome":"exploded","reason":"x"}"#,
            // Since version 8 a job's end says why.
            r#"{"kind":"job.ended","job":4,"outcome":"failed"}"#,
            r#"{"kind":"job.ended","job":4,"outcome":"failed","reason":"Not A Code"}"#,
            r#"{"task":3}"#,
            "not json",
            "",
        ];
        for json in bad {
            assert!(
                Event::decode("job.started", SCHEMA_VERSION, json.as_bytes()).is_err(),
                "{json}"
            );
        }
        let ok = r#"{"kind":"job.started","task":3}"#;
        assert_eq!(
            Event::decode("job.started", SCHEMA_VERSION + 1, ok.as_bytes()),
            Err(EventError::UnsupportedVersion {
                found: SCHEMA_VERSION + 1,
                current: SCHEMA_VERSION
            })
        );
    }
}
