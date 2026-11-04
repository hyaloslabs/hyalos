//! Manifest enforcement, model protocols, forge and egress proxy, and parsing of untrusted input.
//!
//! Layout rule (ADR-0017): this crate depends on no workspace crate except hyalos-types.

mod admission;
pub mod model;
pub mod sandbox;
mod serve;

pub use admission::{Admission, Credential, LiveJobs, Refusal};
pub use serve::{Broker, Exit, ServeError, serve, serve_with};
