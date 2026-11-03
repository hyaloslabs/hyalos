//! Shared domain types: ids, provenance, verdicts, manifests, event envelopes, channel messages. No I/O.
//!
//! Layout rule (ADR-0017): this crate depends on no other workspace crate.

pub mod entropy;
pub mod origin;
pub mod time;

pub use entropy::{Entropy, SeededRng};
pub use origin::Origin;
pub use time::{Clock, Timestamp};
