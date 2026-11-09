//! Verifying a release's Sigstore bundle (S7.12, ADR-0007, ADR-0093).
//!
//! One library does the check: sigstore-verify, pinned to the version spike #73 tested. A bundle is
//! accepted only if all of this holds, with no network and never against the local clock:
//!
//! - the signature is over the artifact, by a certificate that chains to an authority the
//!   trusted-root snapshot lists, valid at every signed time the bundle proves and carrying a
//!   certificate-transparency timestamp from a log the snapshot lists;
//! - the certificate names the pinned identity: its subject, its issuer and the ref it was issued
//!   for;
//! - the bundle holds at least one transparency-log entry, each with its inclusion proof and signed
//!   checkpoint, from a log the snapshot lists;
//! - the bundle holds at least one verified signed time (the log's or a timestamp authority's).
//!
//! The trust is a value: [`Trust::pinned`] is the snapshot built into this release with the
//! pipeline's identity, and it is what an install uses. [`Trust::new`] takes any root and identity,
//! for tests and for the vectors. A refusal is a reason code only: the library's message can carry
//! values from the bundle, which is untrusted, so it is never shown.

use std::fmt;

use hyalos_types::ReasonCode;
use sigstore_verify::trust_root::TrustedRoot;
use sigstore_verify::types::Bundle;
use sigstore_verify::{
    CertificateError, Error, TransparencyLogError, VerificationPolicy, Verifier,
};

/// The most bytes a bundle may hold. A release's bundle is a few kilobytes.
pub const MAX_BUNDLE: u64 = 256 << 10;

/// The trusted-root snapshot this release carries: Sigstore's trusted root as of
/// [`SNAPSHOT_DATE`]. The pipeline's signed snapshot replaces it (S7.11).
pub const SNAPSHOT: &str = include_str!("../trust/trusted_root.json");

/// When [`SNAPSHOT`] was taken.
pub const SNAPSHOT_DATE: &str = "2026-11-02";

/// The identity a plugin release is signed by: the Hyalos marketplace pipeline's release workflow
/// (ADR-0007). Not the plugin author's, whose own CI says nothing about the pipeline's checks.
pub const PIPELINE: Identity<'static> = Identity {
    subject: "https://github.com/hyaloslabs/marketplace/.github/workflows/release.yml@refs/heads/main",
    issuer: "https://token.actions.githubusercontent.com",
    git_ref: Some("refs/heads/main"),
};

/// Who must have signed: the certificate's subject (its SAN), its OIDC issuer and, for a CI
/// workload, the ref the certificate was issued for (Fulcio's `1.3.6.1.4.1.57264.1.14`). Each is
/// compared exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity<'a> {
    pub subject: &'a str,
    pub issuer: &'a str,
    /// `None` only for an identity that is not a CI workload, such as a person's email address.
    pub git_ref: Option<&'a str>,
}

/// A trusted root and the identity it must vouch for, ready to verify bundles.
pub struct Trust {
    verifier: Verifier,
    policy: VerificationPolicy,
    git_ref: Option<String>,
}

/// Why a bundle was refused. Each is a reason code; none carries anything from the bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Larger than [`MAX_BUNDLE`].
    TooLarge,
    /// Not a Sigstore bundle this library reads, or one it does not support.
    Malformed,
    /// The trusted root cannot be used.
    Snapshot,
    /// Signed by another identity: subject, issuer or ref.
    Identity,
    /// The bundle is not about this artifact.
    Digest,
    /// The signature does not verify.
    Signature,
    /// The certificate does not chain to an authority the snapshot lists, was not valid at the
    /// signed time, or has no transparency timestamp from a log the snapshot lists.
    Certificate,
    /// No transparency-log entry, or one without its inclusion proof, a checkpoint that does not
    /// verify, or a log the snapshot does not list.
    Log,
    /// No verified signed time: none in the bundle, or one that does not verify against an
    /// authority the snapshot lists.
    Time,
    /// Anything else the library refused.
    Refused,
}

impl Refusal {
    pub fn code(&self) -> ReasonCode {
        let code = match self {
            Self::TooLarge => "sigstore.too_large",
            Self::Malformed => "sigstore.malformed",
            Self::Snapshot => "sigstore.snapshot",
            Self::Identity => "sigstore.identity",
            Self::Digest => "sigstore.digest",
            Self::Signature => "sigstore.signature",
            Self::Certificate => "sigstore.certificate",
            Self::Log => "sigstore.log",
            Self::Time => "sigstore.time",
            Self::Refused => "sigstore.refused",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "the Sigstore bundle is too large",
            Self::Malformed => "the Sigstore bundle is not one this build reads",
            Self::Snapshot => "the trusted-root snapshot cannot be used",
            Self::Identity => "the release was not signed by the identity this build trusts",
            Self::Digest => "the Sigstore bundle is not about this file",
            Self::Signature => "the release's signature does not verify",
            Self::Certificate => {
                "the signing certificate does not verify against the trusted-root snapshot"
            }
            Self::Log => {
                "the Sigstore bundle has no transparency-log entry that verifies against the trusted-root snapshot"
            }
            Self::Time => "the Sigstore bundle has no signed time that verifies",
            Self::Refused => "the Sigstore bundle does not verify",
        })
    }
}

impl std::error::Error for Refusal {}

impl Trust {
    /// The trust an install uses: [`SNAPSHOT`] and the [`PIPELINE`] identity, both fixed by this
    /// release.
    pub fn pinned() -> Result<Trust, Refusal> {
        Trust::new(SNAPSHOT, PIPELINE)
    }

    /// `root` (a `trusted_root.json`) vouching for `identity`.
    pub fn new(root: &str, identity: Identity<'_>) -> Result<Trust, Refusal> {
        let root = TrustedRoot::from_json(root).map_err(|_| Refusal::Snapshot)?;
        let verifier = Verifier::new(&root).map_err(|_| Refusal::Snapshot)?;
        Ok(Trust {
            verifier,
            policy: VerificationPolicy::new(identity.subject, identity.issuer),
            git_ref: identity.git_ref.map(str::to_string),
        })
    }

    /// Accepts `bundle` for `artifact`, or says why not.
    pub fn verify(&self, artifact: &[u8], bundle: &[u8]) -> Result<(), Refusal> {
        if bundle.len() as u64 > MAX_BUNDLE {
            return Err(Refusal::TooLarge);
        }
        let text = std::str::from_utf8(bundle).map_err(|_| Refusal::Malformed)?;
        let bundle = Bundle::from_json(text).map_err(|_| Refusal::Malformed)?;
        // ADR-0007 asks for these whatever the library does with a bundle that lacks them.
        let entries = &bundle.verification_material.tlog_entries;
        if entries.is_empty() || entries.iter().any(|e| e.inclusion_proof.is_none()) {
            return Err(Refusal::Log);
        }
        let result = self
            .verifier
            .verify(artifact, &bundle, &self.policy)
            .map_err(|e| refusal(&e))?;
        // Every check this policy asks for ran; anything short of that is not an accept.
        if !(result.certificate_verified()
            && result.sct_verified()
            && result.tlog_verified()
            && result.identity_policy_checked())
        {
            return Err(Refusal::Refused);
        }
        if result.verified_timestamps().is_empty() {
            return Err(Refusal::Time);
        }
        if let Some(want) = &self.git_ref {
            let got = result
                .certificate()
                .and_then(|c| c.ci_claims.source_repository_ref.as_deref());
            if got != Some(want.as_str()) {
                return Err(Refusal::Identity);
            }
        }
        Ok(())
    }
}

/// The refusal for the library's error, by kind only.
fn refusal(error: &Error) -> Refusal {
    match error {
        Error::IdentityMismatch { .. } | Error::IssuerMismatch { .. } => Refusal::Identity,
        Error::ArtifactMismatch(_) => Refusal::Digest,
        Error::SignatureInvalid(_) => Refusal::Signature,
        Error::InvalidBundle(_)
        | Error::UnsupportedBundle(_)
        | Error::UnsupportedArtifact(_)
        | Error::Bundle(_)
        | Error::Types(_) => Refusal::Malformed,
        Error::InvalidTrustRoot(_) | Error::TrustRoot(_) | Error::TrustedCertificate(_) => {
            Refusal::Snapshot
        }
        Error::Certificate(CertificateError::Malformed(_)) => Refusal::Malformed,
        Error::Certificate(_) => Refusal::Certificate,
        Error::TransparencyLog(TransparencyLogError::EntryMismatch(_)) => Refusal::Digest,
        Error::TransparencyLog(_) => Refusal::Log,
        Error::Timestamp(_) => Refusal::Time,
        _ => Refusal::Refused,
    }
}
