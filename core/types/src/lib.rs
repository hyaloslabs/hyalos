//! Shared domain types: ids, provenance, verdicts, manifests, event envelopes, channel messages. No I/O.
//!
//! Layout rule (ADR-0017): this crate depends on no other workspace crate.

pub mod digest;
pub mod entropy;
pub mod event;
pub mod hex;
pub mod ids;
pub mod manifest;
pub mod origin;
pub mod reason;
pub mod schema;
pub mod snapshot;
pub mod stakes;
pub mod time;

pub use digest::Digest;
pub use entropy::{Entropy, SeededRng};
pub use event::{Event, EventError, Gate, HoldReason, JobOutcome, Resolution};
pub use ids::{HoldId, JobId, TaskId};
pub use manifest::{
    Approval, Budget, MANIFEST_VERSION, Manifest, ManifestError, ManifestKey, ManifestPublicKey,
    OpenError, Role, SignedManifest,
};
pub use origin::Origin;
pub use reason::{Answer, ReasonCode};
pub use schema::{Schema, Upcast, UpcastFn, Value};
pub use snapshot::{ImageError, STATE_VERSION, StateImage};
pub use stakes::{Sensitive, Stakes, Tier, Touches};
pub use time::{Clock, Timestamp};
