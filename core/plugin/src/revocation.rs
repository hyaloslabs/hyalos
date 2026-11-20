//! The revocation list (S7.7, ADR-0007, ADR-0098): the releases the marketplace maintainers proved
//! malicious, by archive digest, each with a reason code.
//!
//! The file is `{"kind": "hyalos.revocation_list", "revoked": [{"digest": ..., "reason": ...}],
//! "version": 1}`, with its Sigstore bundle beside it, in every release. Its issue time is the
//! earliest signed time the bundle proves. A list is accepted only through [`Revocations::verify`]:
//! its bundle must verify under the trusted-root snapshot in force, for the pipeline's identity,
//! before anything in it is read, and it must be later than the list accepted before it, so an older
//! list can never take a revocation back.
//!
//! Errors are reason codes: nothing from the file or its bundle reaches one.

use std::collections::BTreeMap;

use hyalos_types::{Digest, ReasonCode, Timestamp};

use crate::sigstore::Refusal;
use crate::skill::digest_of;
use crate::snapshot::Snapshot;

/// The list's file name, in a release and in an index location.
pub const FILE: &str = "revocations.json";

/// Its Sigstore bundle's file name, beside it.
pub const BUNDLE: &str = "revocations.json.sigstore.json";

/// What a revocation list file says it is.
pub const KIND: &str = "hyalos.revocation_list";

/// The most bytes a list may hold: some ten thousand entries.
pub const MAX_LIST: u64 = 1 << 20;

/// A revocation list accepted: its file's digest, its issue time and what it revokes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocations {
    digest: Digest,
    signed_at: Timestamp,
    revoked: BTreeMap<Digest, ReasonCode>,
}

/// Why a list was not accepted. Each is a reason code; none carries anything from the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RevocationError {
    /// Not a revocation list of version 1, or larger than [`MAX_LIST`].
    #[error("it is not a revocation list this build reads")]
    Malformed,
    /// It has no Sigstore bundle.
    #[error("it has no Sigstore bundle")]
    Unsigned,
    /// Its bundle does not verify under the snapshot in force, for the pipeline's identity.
    #[error("it does not verify under the trusted-root snapshot in force as the Hyalos marketplace pipeline's ({})", .0.code())]
    Unverifiable(Refusal),
    /// Its issue time is not later than the list accepted before it.
    #[error("it is not newer than the revocation list in force")]
    NotNewer,
}

impl RevocationError {
    pub fn code(&self) -> ReasonCode {
        let code = match self {
            Self::Malformed => "revocations.malformed",
            Self::Unsigned => "revocations.unsigned",
            Self::Unverifiable(_) => "revocations.unverifiable",
            Self::NotNewer => "revocations.not_newer",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

impl Revocations {
    /// `file` with its `bundle`, accepted only if the bundle verifies under `snapshot` for its
    /// identity and the issue time it proves is later than `after`, the list accepted before.
    pub fn verify(
        snapshot: &Snapshot,
        file: &[u8],
        bundle: &[u8],
        after: Option<Timestamp>,
    ) -> Result<Revocations, RevocationError> {
        if file.len() as u64 > MAX_LIST {
            return Err(RevocationError::Malformed);
        }
        let signed_at = snapshot
            .trust()
            .and_then(|trust| trust.verify_signed(file, bundle))
            .map_err(RevocationError::Unverifiable)?;
        if after.is_some_and(|after| signed_at <= after) {
            return Err(RevocationError::NotNewer);
        }
        Revocations::kept(file, signed_at)
    }

    /// A list accepted before: its file as kept, with the issue time the log recorded when it was
    /// imported. The caller checks the file still has the digest the log recorded.
    pub fn kept(file: &[u8], signed_at: Timestamp) -> Result<Revocations, RevocationError> {
        Ok(Revocations {
            digest: digest_of(file),
            signed_at,
            revoked: parse(file)?,
        })
    }

    /// Why the release with archive digest `digest` is revoked, if it is.
    pub fn reason(&self, digest: &Digest) -> Option<&ReasonCode> {
        self.revoked.get(digest)
    }

    /// How many releases it revokes.
    pub fn len(&self) -> usize {
        self.revoked.len()
    }

    /// Whether it revokes none.
    pub fn is_empty(&self) -> bool {
        self.revoked.is_empty()
    }

    /// The digest of the list file.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The earliest signed time its bundle proves.
    pub fn signed_at(&self) -> Timestamp {
        self.signed_at
    }
}

/// What a list file revokes. A digest listed twice, an entry with another field, or a reason that is
/// not a code makes the whole list malformed.
fn parse(file: &[u8]) -> Result<BTreeMap<Digest, ReasonCode>, RevocationError> {
    if file.len() as u64 > MAX_LIST {
        return Err(RevocationError::Malformed);
    }
    let body: serde_json::Value =
        serde_json::from_slice(file).map_err(|_| RevocationError::Malformed)?;
    let fields = |v: &serde_json::Value, keys: &[&str]| {
        v.as_object()
            .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
    };
    if !fields(&body, &["kind", "revoked", "version"])
        || body["kind"].as_str() != Some(KIND)
        || body["version"].as_u64() != Some(1)
    {
        return Err(RevocationError::Malformed);
    }
    let entries = body["revoked"]
        .as_array()
        .ok_or(RevocationError::Malformed)?;
    let mut revoked = BTreeMap::new();
    for entry in entries {
        if !fields(entry, &["digest", "reason"]) {
            return Err(RevocationError::Malformed);
        }
        let digest = entry["digest"]
            .as_str()
            .and_then(Digest::parse)
            .ok_or(RevocationError::Malformed)?;
        let reason = entry["reason"]
            .as_str()
            .and_then(ReasonCode::new)
            .ok_or(RevocationError::Malformed)?;
        if revoked.insert(digest, reason).is_some() {
            return Err(RevocationError::Malformed);
        }
    }
    Ok(revoked)
}
