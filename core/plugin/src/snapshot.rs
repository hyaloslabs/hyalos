//! The trusted-root snapshot (S7.11, ADR-0007, ADR-0097): Sigstore's trusted root as the marketplace
//! pipeline fetched it through TUF, signed by the pipeline so that it carries a signed issue time.
//!
//! The file is `{"kind": "hyalos.trusted_root_snapshot", "trusted_root": {...}, "version": 1}`, with
//! its Sigstore bundle beside it. Its issue time is the earliest signed time the bundle proves; the
//! file holds no time of its own. A release embeds one ([`Snapshot::embedded`]). A newer one is
//! accepted only through [`Snapshot::successor`]: its bundle must verify under the snapshot in force,
//! for the pipeline's identity, and it must be later than that snapshot. Nothing else makes a
//! snapshot trusted, so a snapshot can be replaced only by a newer one the old one vouches for.
//!
//! Errors are reason codes: nothing from the file or its bundle reaches one.

use std::fmt;

use hyalos_types::{Digest, ReasonCode, Timestamp};

use crate::sigstore::{Identity, PIPELINE, Refusal, Trust};
use crate::skill::digest_of;

/// The snapshot's file name, in a release and in an index location.
pub const FILE: &str = "trusted-root.json";

/// Its Sigstore bundle's file name, beside it.
pub const BUNDLE: &str = "trusted-root.json.sigstore.json";

/// What a snapshot file says it is.
pub const KIND: &str = "hyalos.trusted_root_snapshot";

/// The most bytes a snapshot may hold. Sigstore's trusted root is a few kilobytes.
pub const MAX_SNAPSHOT: u64 = 1 << 20;

/// The snapshot this release embeds, as the pipeline published it, and its bundle.
const EMBEDDED: &[u8] = include_bytes!("../trust/trusted-root.json");
const EMBEDDED_BUNDLE: &[u8] = include_bytes!("../trust/trusted-root.json.sigstore.json");

/// A trusted-root snapshot: the root to verify against, the identity it must vouch for, the file's
/// digest and its issue time.
#[derive(Clone)]
pub struct Snapshot {
    root: String,
    identity: Identity<'static>,
    digest: Digest,
    signed_at: Timestamp,
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("digest", &self.digest)
            .field("signed_at", &self.signed_at)
            .finish_non_exhaustive()
    }
}

/// Why a snapshot was not accepted. Each is a reason code; none carries anything from the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not a snapshot of version 1, or larger than [`MAX_SNAPSHOT`].
    Malformed,
    /// It has no Sigstore bundle.
    Unsigned,
    /// Its bundle does not verify under the snapshot in force, for the pipeline's identity.
    Unverifiable(Refusal),
    /// Its issue time is not later than the snapshot in force's.
    NotNewer,
    /// It holds a trusted root this build cannot use.
    Unusable,
}

impl SnapshotError {
    pub fn code(&self) -> ReasonCode {
        let code = match self {
            Self::Malformed => "snapshot.malformed",
            Self::Unsigned => "snapshot.unsigned",
            Self::Unverifiable(_) => "snapshot.unverifiable",
            Self::NotNewer => "snapshot.not_newer",
            Self::Unusable => "snapshot.unusable",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => f.write_str("it is not a trusted-root snapshot this build reads"),
            Self::Unsigned => f.write_str("it has no Sigstore bundle"),
            Self::Unverifiable(refusal) => write!(
                f,
                "it does not verify under the trusted-root snapshot in force ({}): if the Hyalos marketplace pipeline signed it, Sigstore has changed beyond what that snapshot can follow, and a new Hyalos release is needed",
                refusal.code()
            ),
            Self::NotNewer => {
                f.write_str("it is not newer than the trusted-root snapshot in force")
            }
            Self::Unusable => f.write_str("its trusted root cannot be used"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl Snapshot {
    /// The snapshot this release embeds. Its bundle verifies under its own root for the pipeline's
    /// identity, and that gives its issue time.
    pub fn embedded() -> Result<Snapshot, SnapshotError> {
        let root = unwrap(EMBEDDED)?;
        let trust = Trust::new(&root, PIPELINE).map_err(|_| SnapshotError::Unusable)?;
        let signed_at = trust
            .verify_signed(EMBEDDED, EMBEDDED_BUNDLE)
            .map_err(SnapshotError::Unverifiable)?;
        Ok(Snapshot {
            root,
            identity: PIPELINE,
            digest: digest_of(EMBEDDED),
            signed_at,
        })
    }

    /// A snapshot accepted before: its file as kept, with the issue time the log recorded when it was
    /// imported. The caller checks the file still has the digest the log recorded.
    pub fn kept(file: &[u8], signed_at: Timestamp) -> Result<Snapshot, SnapshotError> {
        let root = unwrap(file)?;
        Trust::new(&root, PIPELINE).map_err(|_| SnapshotError::Unusable)?;
        Ok(Snapshot {
            root,
            identity: PIPELINE,
            digest: digest_of(file),
            signed_at,
        })
    }

    /// A trusted root (a `trusted_root.json`, not a snapshot file) trusted as of `signed_at` for
    /// `identity`, by other means than a signature. For tests, which sign with a Sigstore of their own;
    /// the binary trusts only [`Snapshot::embedded`] and what it vouches for.
    pub fn trusted(
        root: &str,
        identity: Identity<'static>,
        signed_at: Timestamp,
    ) -> Result<Snapshot, SnapshotError> {
        Trust::new(root, identity).map_err(|_| SnapshotError::Unusable)?;
        Ok(Snapshot {
            root: root.to_string(),
            identity,
            digest: digest_of(root.as_bytes()),
            signed_at,
        })
    }

    /// `file` with its `bundle`, as the snapshot after this one: accepted only if the bundle verifies
    /// under this snapshot for its identity, the issue time it proves is later than this one's, and
    /// the root inside can be used.
    pub fn successor(&self, file: &[u8], bundle: &[u8]) -> Result<Snapshot, SnapshotError> {
        if file.len() as u64 > MAX_SNAPSHOT {
            return Err(SnapshotError::Malformed);
        }
        let signed_at = self
            .trust()
            .map_err(|_| SnapshotError::Unusable)?
            .verify_signed(file, bundle)
            .map_err(SnapshotError::Unverifiable)?;
        if signed_at <= self.signed_at {
            return Err(SnapshotError::NotNewer);
        }
        let root = unwrap(file)?;
        Trust::new(&root, self.identity).map_err(|_| SnapshotError::Unusable)?;
        Ok(Snapshot {
            root,
            identity: self.identity,
            digest: digest_of(file),
            signed_at,
        })
    }

    /// The trust to verify releases with: this root, for this snapshot's identity.
    pub fn trust(&self) -> Result<Trust, Refusal> {
        Trust::new(&self.root, self.identity)
    }

    /// The trusted root inside, as `trusted_root.json`.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// The digest of the snapshot file.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The earliest signed time its bundle proves.
    pub fn signed_at(&self) -> Timestamp {
        self.signed_at
    }
}

/// The trusted root a snapshot file holds, as JSON.
fn unwrap(file: &[u8]) -> Result<String, SnapshotError> {
    if file.len() as u64 > MAX_SNAPSHOT {
        return Err(SnapshotError::Malformed);
    }
    let body: serde_json::Value =
        serde_json::from_slice(file).map_err(|_| SnapshotError::Malformed)?;
    let ok = body.get("kind").and_then(|k| k.as_str()) == Some(KIND)
        && body.get("version").and_then(|v| v.as_u64()) == Some(1)
        && body.get("trusted_root").is_some_and(|r| r.is_object());
    if !ok {
        return Err(SnapshotError::Malformed);
    }
    // The library accepts a root that lists nothing; such a root would verify nothing, and is no
    // snapshot of Sigstore's.
    let root = &body["trusted_root"];
    let lists = |field: &str| {
        root.get(field)
            .and_then(|f| f.as_array())
            .is_some_and(|a| !a.is_empty())
    };
    if !(lists("tlogs") && lists("certificateAuthorities") && lists("ctlogs")) {
        return Err(SnapshotError::Unusable);
    }
    serde_json::to_string(root).map_err(|_| SnapshotError::Malformed)
}
