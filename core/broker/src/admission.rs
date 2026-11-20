//! Admitting a job: the broker's check of its signed manifest (ADR-0004, ADR-0031).
//!
//! The broker enforces the manifest and nothing else, so before it serves a job it decides whether
//! the manifest in front of it is one it may enforce. A manifest is admitted only if it is the
//! engine's, in a version this build knows, for the job presenting it, for a job that is live, for the
//! role of the credential used, and not past its deadline. Anything else is refused, and a refusal
//! carries a reason and nothing from inside the document.

use hyalos_types::{
    Clock, JobId, Manifest, ManifestPublicKey, OpenError, ReasonCode, Role, SignedManifest,
    Timestamp,
};

/// What a sandbox presents to the broker. The broker issued it for one job at one role (a reader's
/// credential is a reader's, ADR-0005), so its role is the broker's own record and not the
/// sandbox's claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credential {
    pub job: JobId,
    pub role: Role,
}

/// Which jobs are running. The engine tells the broker when a job starts and ends; the broker never
/// guesses.
pub trait LiveJobs {
    fn is_live(&self, job: JobId) -> bool;
}

/// Why a job was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Not the engine's manifest, not a version this build knows, or not a manifest at all.
    #[error("manifest refused: {0}")]
    Manifest(OpenError),
    /// The credential is for one job and the manifest for another.
    #[error(
        "manifest refused: it is for job {manifest} and the credential is for job {credential}"
    )]
    WrongJob { manifest: JobId, credential: JobId },
    /// The job is not one the engine has started and not ended: a replayed or stale manifest.
    #[error("manifest refused: job {0} is not a live job")]
    JobNotLive(JobId),
    /// The manifest names a different role than the credential in use.
    #[error(
        "manifest refused: it is for role {manifest:?} and the credential is for role {credential:?}"
    )]
    RoleMismatch { manifest: Role, credential: Role },
    /// Past the deadline.
    #[error("manifest refused: its deadline {} has passed (now {})", .deadline.as_millis(), .now.as_millis())]
    Expired { deadline: Timestamp, now: Timestamp },
}

impl Refusal {
    /// The short code the engine is told, which says which check failed and nothing from the
    /// document or the credential.
    pub fn reason(&self) -> ReasonCode {
        let code = match self {
            Refusal::Manifest(OpenError::UnknownVersion { .. }) => "manifest.unknown_version",
            Refusal::Manifest(OpenError::BadSignature) => "manifest.bad_signature",
            Refusal::Manifest(OpenError::Malformed) => "manifest.malformed",
            Refusal::Manifest(OpenError::VersionMismatch { .. }) => "manifest.version_mismatch",
            Refusal::WrongJob { .. } => "admission.wrong_job",
            Refusal::JobNotLive(_) => "admission.job_not_live",
            Refusal::RoleMismatch { .. } => "admission.role_mismatch",
            Refusal::Expired { .. } => "admission.expired",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

/// Admits jobs by the engine's public manifest key, which the engine hands the broker when it
/// launches it (ADR-0022).
#[derive(Debug, Clone, Copy)]
pub struct Admission {
    key: ManifestPublicKey,
}

impl Admission {
    pub fn new(key: ManifestPublicKey) -> Self {
        Self { key }
    }

    /// The manifest to enforce for this job, or why there is none. The checks run in this order, so
    /// the first one that fails is the reason given; none of them reads anything the signature has
    /// not already covered.
    pub fn admit(
        &self,
        signed: &SignedManifest,
        credential: &Credential,
        jobs: &dyn LiveJobs,
        clock: &dyn Clock,
    ) -> Result<Manifest, Refusal> {
        let manifest = signed.open(&self.key).map_err(Refusal::Manifest)?;
        if manifest.job != credential.job {
            return Err(Refusal::WrongJob {
                manifest: manifest.job,
                credential: credential.job,
            });
        }
        if !jobs.is_live(manifest.job) {
            return Err(Refusal::JobNotLive(manifest.job));
        }
        if manifest.role != credential.role {
            return Err(Refusal::RoleMismatch {
                manifest: manifest.role,
                credential: credential.role,
            });
        }
        // The deadline itself is still valid, as an answer exactly at a gate's deadline is on time.
        let now = clock.now();
        if now > manifest.deadline {
            return Err(Refusal::Expired {
                deadline: manifest.deadline,
                now,
            });
        }
        Ok(manifest)
    }
}
