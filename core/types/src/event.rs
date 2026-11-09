//! The events that tasks, jobs and holds are rebuilt from (ADR-0003, ADR-0009, ADR-0019).
//!
//! Events hold typed values, digests and references, never credentials, tokens or raw text. Each one is
//! stored as JSON with a `kind` tag that repeats the log's `kind` column. Decoding refuses unknown
//! fields, an unknown kind and an unknown schema version, so a payload the engine does not understand
//! is an error and never a guess.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, AgentName, Answer, Attestation, CommitSha, Digest, Grant, HoldId, InstallPolicy,
    JobId, MergeMethod, Origin, PluginName, PluginSource, ReasonCode, Schema, Signed, Stakes,
    SubstrateName, TaskId, TypedValue, Upcast, Value,
};

/// The version these events are written with. Older versions are read by upcasting (ADR-0023).
pub const SCHEMA_VERSION: u32 = 17;

/// What this build reads: version 15, and versions 1 to 14 upcast to it (ADR-0023, ADR-0025, ADR-0027,
/// ADR-0029, ADR-0034, ADR-0043, ADR-0050, ADR-0053, ADR-0057, ADR-0063, ADR-0068, ADR-0069,
/// ADR-0070, ADR-0071, ADR-0089).
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
/// 17 added `plugin.policy_set` and changed no payload.
pub static SCHEMA: Schema = Schema {
    current: SCHEMA_VERSION,
    steps: &[
        Upcast {
            kind: "hold.opened",
            from: 1,
            step: hold_opened_v1,
        },
        Upcast {
            kind: "hold.resolved",
            from: 1,
            step: hold_resolved_v1,
        },
        Upcast {
            kind: "job.started",
            from: 5,
            step: job_started_v5,
        },
        Upcast {
            kind: "job.started",
            from: 6,
            step: job_started_v6,
        },
        Upcast {
            kind: "job.ended",
            from: 7,
            step: job_ended_v7,
        },
        Upcast {
            kind: "job.started",
            from: 8,
            step: job_started_v8,
        },
        Upcast {
            kind: "hold.resolved",
            from: 12,
            step: decision_v12,
        },
        Upcast {
            kind: "task.cancelled",
            from: 12,
            step: decision_v12,
        },
        Upcast {
            kind: "policy.approved",
            from: 12,
            step: decision_v12,
        },
        Upcast {
            kind: "handle.released",
            from: 12,
            step: decision_v12,
        },
        Upcast {
            kind: "agent.issued",
            from: 12,
            step: decision_v12,
        },
        Upcast {
            kind: "agent.revoked",
            from: 12,
            step: decision_v12,
        },
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
    /// A human released the raw text the broker holds as `handle` to the task's actor (ADR-0005,
    /// ADR-0063). The handle is the SHA-256 of the bytes the broker holds, so the decision is bound to
    /// exactly that text: the engine never sees it, and the broker serves only bytes with this digest.
    /// Raw forge text reaching an actor is a decision only a human makes (ADR-0012), and the state
    /// refuses anyone else.
    #[serde(rename = "handle.released")]
    HandleReleased {
        task: TaskId,
        handle: Digest,
        by: Resolution,
        reason: ReasonCode,
        signed: Signed,
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
    /// The typed values a running job returned: a reader's or planner's answer, or an actor's submitted
    /// result (ADR-0062, ADR-0064, ADR-0069). A choice is its option's index, so no text is stored. It
    /// is written together with the job's ending, so a resumed run finds every finished step's values
    /// in the log.
    #[serde(rename = "job.values")]
    JobValues { job: JobId, values: Vec<TypedValue> },
}

/// The largest policy document an event may hold, in bytes. Events are read whole into memory by
/// replay, so a document is bounded.
pub const MAX_POLICY_DOCUMENT: usize = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum EventError {
    Encode(String),
    Decode(String),
    /// The row's schema version is 0 or newer than this build reads (ADR-0023).
    UnsupportedVersion {
        found: u32,
        current: u32,
    },
    /// An upcast step refused the payload it was given.
    Upcast {
        kind: String,
        from: u32,
        reason: String,
    },
    /// The log's `kind` column and the payload's own `kind` disagree.
    KindMismatch {
        column: String,
        payload: &'static str,
    },
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventError::Encode(e) => write!(f, "cannot encode the event: {e}"),
            EventError::Decode(e) => write!(f, "cannot decode the event: {e}"),
            EventError::UnsupportedVersion { found: 0, .. } => {
                write!(
                    f,
                    "event schema version 0 is not valid; versions start at 1"
                )
            }
            EventError::UnsupportedVersion { found, current } => write!(
                f,
                "event schema version {found} was written by a newer Hyalos; this build reads up to version {current}. Upgrade Hyalos to continue this log (there is no downgrade)"
            ),
            EventError::Upcast { kind, from, reason } => write!(
                f,
                "cannot bring a `{kind}` event written at schema version {from} up to date: {reason}"
            ),
            EventError::KindMismatch { column, payload } => write!(
                f,
                "the log says this is a `{column}` event but its payload is a `{payload}` event"
            ),
        }
    }
}

impl std::error::Error for EventError {}

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
            | Event::JobValues { .. } => origin == Origin::Engine,
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

    /// The value for the log's `kind` column.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::TaskCreated { .. } => "task.created",
            Event::TaskCompleted { .. } => "task.completed",
            Event::JobStarted { .. } => "job.started",
            Event::JobEnded { .. } => "job.ended",
            Event::GateAnswered { .. } => "gate.answered",
            Event::HoldOpened { .. } => "hold.opened",
            Event::HoldResolved { .. } => "hold.resolved",
            Event::ArchivePruned { .. } => "archive.pruned",
            Event::TaskCancelled { .. } => "task.cancelled",
            Event::HoldRefused { .. } => "hold.refused",
            Event::PolicyApproved { .. } => "policy.approved",
            Event::HandleReleased { .. } => "handle.released",
            Event::AgentIssued { .. } => "agent.issued",
            Event::AgentRevoked { .. } => "agent.revoked",
            Event::MergeApproved { .. } => "merge.approved",
            Event::PluginInstalled { .. } => "plugin.installed",
            Event::PluginPolicySet { .. } => "plugin.policy_set",
            Event::JobValues { .. } => "job.values",
        }
    }

    /// The bytes stored in the log's `payload` column.
    pub fn encode(&self) -> Result<Vec<u8>, EventError> {
        serde_json::to_vec(self).map_err(|e| EventError::Encode(e.to_string()))
    }

    /// Every `kind` value, for checking tables and test coverage.
    pub const KINDS: [&'static str; 18] = [
        "task.created",
        "task.completed",
        "job.started",
        "job.ended",
        "gate.answered",
        "hold.opened",
        "hold.resolved",
        "archive.pruned",
        "task.cancelled",
        "hold.refused",
        "policy.approved",
        "handle.released",
        "agent.issued",
        "agent.revoked",
        "merge.approved",
        "plugin.installed",
        "plugin.policy_set",
        "job.values",
    ];

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
