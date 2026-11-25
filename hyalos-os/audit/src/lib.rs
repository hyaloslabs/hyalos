//! The audit of a hyalOS record: its types, identities, replay, chain, log and answer checks, and
//! the report. The monitor's `audit` command and `hyalos-verify` are both thin callers of it.
//!
//! It holds no KVM, live model call, notary service or link signing. Everything is checked before
//! a report exists, and a check that could not finish is never a pass.

#![forbid(unsafe_code)]

pub mod answers;
pub mod audit;
pub mod chain;
#[cfg(feature = "spike-contract")]
pub mod contract;
pub mod failure;
pub mod files;
pub mod record;
pub mod replay;
pub mod report;

pub use answers::{Attempt, Verifier, Witnessed, check_answer, witnessed};
pub use audit::{AuditInputs, Verdict, audit_record, check, claim_of, notary_key_text};
pub use chain::{Audited, Auditor, Claim, LogSource, REKOR_KEY, public_der, signature_verifies};
#[cfg(feature = "spike-contract")]
pub use contract::{Contract, MAX_CONTRACT};
pub use failure::{Failure, FailureKind};
pub use files::{FileError, FileKind, Files, PlainFiles};
pub use record::*;
pub use replay::{
    Asked, Host, ReplaySource, Source, execute_with, in_process, replay, replay_with,
};
pub use report::{HIDDEN_BYTES_LIMIT, attested_report, audit_report, utc};

/// True in a build that can read the stand-in trust settings.
pub const STAND_IN_BUILD: bool = cfg!(feature = "test-trust");
