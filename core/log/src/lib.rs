//! The event log: append-only, hash-chained and signed, in SQLite (ADR-0003, ADR-0018).
//!
//! Only the engine opens a [`Log`] for writing. The broker has no dependency on this crate, which
//! `cargo xtask layout` enforces, so it cannot link the code that opens the database or holds the key.
//! Events the broker produces reach the log as [`Origin::Broker`] events appended by the engine.
//!
//! Layout rule (ADR-0017): this crate depends only on `hyalos-types`.

pub mod format;
mod key;
mod store;
mod verify;

pub use hyalos_types::Origin;
pub use key::{LogKey, PublicKey};
pub use store::{Head, Log, LogError, StoredEvent};
pub use verify::{Reason, Verified, VerifyError, verify};
