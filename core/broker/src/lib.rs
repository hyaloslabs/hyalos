//! Manifest enforcement, model protocols, the git and egress proxies, and parsing of untrusted input.
//!
//! Layout rule (ADR-0017): this crate depends on no workspace crate except hyalos-types.

mod admission;
pub mod egress;
pub mod git;
pub mod model;
pub mod sandbox;
mod serve;

pub use admission::{Admission, Credential, LiveJobs, Refusal};
pub use serve::{Broker, Exit, ServeError, serve, serve_with};
