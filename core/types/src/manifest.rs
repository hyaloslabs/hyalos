//! The job manifest: what the engine signs and the broker enforces (ADR-0004, ADR-0031).
//!
//! A manifest is the whole of a job's authority. The engine signs it; the broker checks the
//! signature with the engine's public manifest key and enforces the document and nothing else. This
//! module holds the document, the signing key, the public key and the signed envelope, so the engine
//! (which signs) and the broker (which checks) share one definition and neither needs the other.
//!
//! The signed bytes are the manifest's JSON. It is built from a struct and from sorted sets, so the
//! same manifest is always the same bytes, and the signature covers a tag, the schema version and
//! those bytes. The version is also outside the payload, so a verifier can refuse a version it does
//! not know before parsing anything.

use std::collections::BTreeSet;
use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::Digest;
use crate::hex;
use crate::ids::JobId;
use crate::merge::{CommitSha, MergeMethod};
use crate::time::Timestamp;
use crate::workflow::Output;

/// The only manifest schema this build writes and reads. A field that widens authority needs an ADR
/// and a new version (ADR-0004). Version 2 gave the approval its pull request and merge method
/// (ADR-0071).
pub const MANIFEST_VERSION: u32 = 2;

/// What a manifest signature is made under, so it cannot be replayed as any other signature the
/// engine makes (an event's, a snapshot's).
const SIGNATURE_TAG: &[u8] = b"hyalos.manifest.signature\0";

pub const SIGNATURE_LEN: usize = 64;

/// What a step is, as far as the broker needs to know (ADR-0005): a reader's credential refuses
/// tools, so the broker learns the role from the manifest and checks it against the credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Reader,
    Planner,
    Actor,
    Deterministic,
}

impl Role {
    pub const ALL: [Role; 4] = [
        Role::Reader,
        Role::Planner,
        Role::Actor,
        Role::Deterministic,
    ];
}

/// How much a job may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub tokens: u64,
}

/// A human's merge approval, carried inside the manifest so the broker, which enforces only the
/// manifest, can see it (ADR-0004, ADR-0012, ADR-0071). It names the `merge.approved` event and
/// repeats what that event approved: pull request `pr` of the manifest's repository, merged by
/// `method`, only while its head is `head_sha`. The engine mints it only from the event itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// The sequence number of the approval event in the log.
    pub event: u64,
    pub pr: u64,
    /// The head commit of the pull request the approval is for.
    pub head_sha: CommitSha,
    pub method: MergeMethod,
}

/// One job's authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The schema version this document is written in.
    pub version: u32,
    pub job: JobId,
    pub role: Role,
    /// Derived by the engine from the step's inputs, never requested (ADR-0004, ADR-0005).
    pub tainted: bool,
    pub repo: String,
    pub branch_prefix: String,
    /// The branch a pull request the job opens may target (ADR-0048). Absent, the job opens none. It
    /// is left out of the signed bytes when absent, so a manifest without it signs as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_base: Option<String>,
    pub actions: BTreeSet<String>,
    pub egress_hosts: BTreeSet<String>,
    pub tools: BTreeSet<String>,
    /// Where the job's data may go, from [`flow`].
    pub data_flows: BTreeSet<String>,
    pub budget: Budget,
    /// The last moment this manifest is valid. A job whose deadline passes ends (ADR-0004).
    pub deadline: Timestamp,
    pub approval: Option<Approval>,
    /// The handles whose raw text the job may read from the broker, each released to its task by a
    /// human (`handle.released`, ADR-0063). Left out of the signed bytes when empty, so a manifest
    /// without any signs as before.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub handles: BTreeSet<Digest>,
    /// The typed result the job's harness must submit through `hyalos-submit`, which the broker
    /// checks it against (ADR-0064). Absent, the job submits nothing, and the field is left out of
    /// the signed bytes, so a manifest without one signs as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Output>,
    /// The issue of the manifest's repository the job fixes, from the run's own request, never from
    /// forge text. A pull request the job opens says it closes it, in a line the broker adds
    /// (ADR-0083). Absent, the broker adds nothing, and the field is left out of the signed bytes, so a
    /// manifest without one signs as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closes: Option<u64>,
}

/// The data flows a manifest may declare (ADR-0055). A flow names where the job's data may go: the
/// broker sees what leaves the sandbox, never which of what the sandbox read it came from, so every
/// flow's source is the job. The broker refuses a call to a sink whose flow is not declared.
pub mod flow {
    /// Model calls on the job's socket.
    pub const MODEL: &str = "job->model";
    /// Writes to the forge: a push through the git proxy, a pull request.
    pub const FORGE: &str = "job->forge";
    /// Requests to web hosts through the egress gate.
    pub const WEB: &str = "job->web";
    /// Every flow there is.
    pub const ALL: [&str; 3] = [MODEL, FORGE, WEB];

    /// Whether `name` is a flow this build knows.
    pub fn known(name: &str) -> bool {
        ALL.contains(&name)
    }
}

/// Why bytes are not a manifest this build can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Not a signed manifest envelope.
    NotAnEnvelope(String),
    /// The signed payload is not a manifest.
    NotAManifest(String),
    /// The manifest could not be written out.
    Encode(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::NotAnEnvelope(e) => write!(f, "not a signed manifest: {e}"),
            ManifestError::NotAManifest(e) => {
                write!(f, "the signed payload is not a manifest: {e}")
            }
            ManifestError::Encode(e) => write!(f, "cannot write the manifest: {e}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Why a signed manifest was not opened. Each variant is a reason to refuse the job; none says what
/// was in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// A schema version this build does not know. Checked before anything else is read.
    UnknownVersion { found: u32 },
    /// The signature is not the engine's over this version and payload.
    BadSignature,
    /// The signature is good but the payload is not a manifest.
    Malformed,
    /// The version inside the document is not the one it was signed and sent under.
    VersionMismatch { envelope: u32, document: u32 },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::UnknownVersion { found } => write!(
                f,
                "manifest version {found} is not one this build knows ({MANIFEST_VERSION})"
            ),
            OpenError::BadSignature => f.write_str("the manifest signature is not valid"),
            OpenError::Malformed => f.write_str("the signed payload is not a manifest"),
            OpenError::VersionMismatch { envelope, document } => write!(
                f,
                "the manifest was sent as version {envelope} but says it is version {document}"
            ),
        }
    }
}

impl std::error::Error for OpenError {}

/// A manifest, signed. The payload is exactly the bytes that were signed.
#[derive(Clone, PartialEq, Eq)]
pub struct SignedManifest {
    version: u32,
    payload: Vec<u8>,
    signature: [u8; SIGNATURE_LEN],
}

#[derive(Serialize, Deserialize)]
struct Wire {
    version: u32,
    /// The manifest's JSON, as hex.
    manifest: String,
    signature: String,
}

impl SignedManifest {
    /// Builds an envelope from parts, as a test of a broker would. Nothing is checked until
    /// [`SignedManifest::open`].
    pub fn from_parts(version: u32, payload: Vec<u8>, signature: [u8; SIGNATURE_LEN]) -> Self {
        Self {
            version,
            payload,
            signature,
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn signature(&self) -> &[u8; SIGNATURE_LEN] {
        &self.signature
    }

    /// The envelope as it travels to the broker.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ManifestError> {
        serde_json::to_vec(&Wire {
            version: self.version,
            manifest: hex::to_hex(&self.payload),
            signature: hex::to_hex(&self.signature),
        })
        .map_err(|e| ManifestError::Encode(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ManifestError> {
        let wire: Wire = serde_json::from_slice(bytes)
            .map_err(|e| ManifestError::NotAnEnvelope(e.to_string()))?;
        let signature = hex::from_hex::<SIGNATURE_LEN>(&wire.signature).ok_or_else(|| {
            ManifestError::NotAnEnvelope("the signature is not 64 bytes of hex".into())
        })?;
        let payload = decode_hex(&wire.manifest)
            .ok_or_else(|| ManifestError::NotAnEnvelope("the manifest is not hex".into()))?;
        Ok(Self {
            version: wire.version,
            payload,
            signature,
        })
    }

    /// Checks the version, then the signature, and only then reads the document. A manifest comes
    /// out only if this build knows its version, the engine signed exactly these bytes, and the
    /// document agrees with the version it was sent under.
    pub fn open(&self, key: &ManifestPublicKey) -> Result<Manifest, OpenError> {
        if self.version != MANIFEST_VERSION {
            return Err(OpenError::UnknownVersion {
                found: self.version,
            });
        }
        if !key.verify(self.version, &self.payload, &self.signature) {
            return Err(OpenError::BadSignature);
        }
        let manifest: Manifest =
            serde_json::from_slice(&self.payload).map_err(|_| OpenError::Malformed)?;
        if manifest.version != self.version {
            return Err(OpenError::VersionMismatch {
                envelope: self.version,
                document: manifest.version,
            });
        }
        Ok(manifest)
    }
}

// The payload is the authority being granted; print its size, not it.
impl fmt::Debug for SignedManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedManifest")
            .field("version", &self.version)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        out.push(hex::from_hex::<1>(pair)?[0]);
    }
    Some(out)
}

/// What is signed: the tag, the version, then the payload. Binding the version here means an
/// envelope cannot be relabelled with a version it was not signed under.
fn signing_message(version: u32, payload: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(SIGNATURE_TAG.len() + 4 + payload.len());
    m.extend_from_slice(SIGNATURE_TAG);
    m.extend_from_slice(&version.to_be_bytes());
    m.extend_from_slice(payload);
    m
}

/// The engine's manifest signing key. It lives in the engine's memory for one run and is never
/// written (ADR-0022): a manifest cannot outlive the engine that signed it. The broker gets only the
/// public half.
pub struct ManifestKey(SigningKey);

impl ManifestKey {
    /// The binary draws the seed from the operating system; this crate has no entropy source.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }

    pub fn public(&self) -> ManifestPublicKey {
        ManifestPublicKey(self.0.verifying_key().to_bytes())
    }

    /// Signs `manifest` under the current schema version. A manifest written in another version is
    /// refused rather than signed, so nothing is signed that the broker would then have to refuse.
    pub fn sign(&self, manifest: &Manifest) -> Result<SignedManifest, ManifestError> {
        if manifest.version != MANIFEST_VERSION {
            return Err(ManifestError::Encode(format!(
                "the manifest says version {}, and this build signs version {MANIFEST_VERSION}",
                manifest.version
            )));
        }
        let payload =
            serde_json::to_vec(manifest).map_err(|e| ManifestError::Encode(e.to_string()))?;
        let signature = self
            .0
            .sign(&signing_message(MANIFEST_VERSION, &payload))
            .to_bytes();
        Ok(SignedManifest {
            version: MANIFEST_VERSION,
            payload,
            signature,
        })
    }
}

// A derived Debug would be one field away from printing the secret.
impl fmt::Debug for ManifestKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ManifestKey(public {})", self.public().to_hex())
    }
}

/// The public half the broker checks manifests with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestPublicKey([u8; 32]);

impl ManifestPublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::to_hex(&self.0)
    }

    /// Strict verification: it also refuses malleable signatures and weak keys.
    fn verify(&self, version: u32, payload: &[u8], signature: &[u8; SIGNATURE_LEN]) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        key.verify_strict(
            &signing_message(version, payload),
            &Signature::from_bytes(signature),
        )
        .is_ok()
    }
}
