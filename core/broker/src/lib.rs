//! Manifest enforcement, model protocols, the git and egress proxies, and parsing of untrusted input.
//!
//! Layout rule (ADR-0017): this crate depends on no workspace crate except hyalos-types.

mod admission;
pub mod bodies;
pub mod content;
pub mod egress;
pub mod forge;
pub mod git;
pub mod handles;
pub mod model;
pub mod pack;
pub mod pulls;
pub mod reader;
pub mod sandbox;
mod serve;
pub mod submit;

pub use admission::{Admission, Credential, LiveJobs, Refusal};
pub use serve::{Broker, Exit, Plugins, ServeError, serve, serve_with, serve_with_plugins};
