//! Shared domain types: ids, provenance, verdicts, manifests, event envelopes, channel messages. No I/O.
//!
//! Layout rule (ADR-0017): this crate depends on no other workspace crate.

pub mod digest;
pub mod entropy;
pub mod event;
pub mod forge;
pub mod hex;
pub mod ids;
pub mod manifest;
pub mod origin;
pub mod prompt;
pub mod provenance;
pub mod reason;
pub mod schema;
pub mod snapshot;
pub mod stakes;
pub mod substrate;
pub mod time;
pub mod wire;
pub mod workflow;

pub use digest::Digest;
pub use entropy::{Entropy, SeededRng};
pub use event::{
    CredentialTier, Event, EventError, Gate, HoldReason, JobOutcome, MAX_POLICY_DOCUMENT,
    Resolution,
};
pub use forge::ForgeToken;
pub use ids::{HoldId, JobId, TaskId};
pub use manifest::{
    Approval, Budget, MANIFEST_VERSION, Manifest, ManifestError, ManifestKey, ManifestPublicKey,
    OpenError, Role, SignedManifest, flow,
};
pub use origin::Origin;
pub use prompt::{ActorPrompt, Choice, Choices, ReaderPrompt, Sourced, Typed};
pub use provenance::{
    ForgeIssue, ForgeText, IssueState, ModelText, OperatorText, PluginText, Provenance, StepInputs,
};
pub use reason::{Answer, ReasonCode};
pub use schema::{Schema, Upcast, UpcastFn, Value};
pub use snapshot::{HandleImage, ImageError, PolicyImage, STATE_VERSION, StateImage};
pub use stakes::{Sensitive, Stakes, Tier, Touches};
pub use substrate::{Attestation, SubstrateName};
pub use time::{Clock, Timestamp};
pub use workflow::{Checked, TypedValue, Workflow, WorkflowError};
