//! The operator's signature on a human decision (ADR-0003, ADR-0012, ADR-0070).
//!
//! Every event that records a human's decision carries [`Signed::Operator`]: an Ed25519 signature by
//! the operator's own key over the event, the engine's public key and the last event the operator
//! saw. The engine signs every event too, but its key alone cannot make a human decision: the log
//! shows who decided, and `hyalos log verify` refuses a human decision the operator did not sign.
//!
//! What is signed binds the decision to one log and one place in it. The engine's public key names
//! the log. `after` and `after_hash` name the last event the operator's command saw, and the state
//! requires each approval's `after` to be at or past the previous approval's own event, so an old
//! approval copied forward is refused. The event itself is signed with [`Signed::None`] in place of
//! the signature, so the signature covers every other field.
//!
//! Keys come in as bytes: this crate has no entropy source (ADR-0013). The binary keeps the operator's
//! key encrypted under a passphrase that the driving agent does not know.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::de::{Deserializer, Error};
use serde::{Deserialize, Serialize, Serializer};

use crate::{Digest, hex};

/// The domain tag every approval signature starts with, so it can never be read as another message
/// of Hyalos's.
const APPROVAL_TAG: &[u8] = b"hyalos.approval.signature";

pub const APPROVAL_SIGNATURE_LEN: usize = 64;

/// The operator's signing key. A derived `Debug` would be one field away from printing it.
pub struct OperatorKey(SigningKey);

impl OperatorKey {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }

    pub fn public(&self) -> OperatorPublic {
        OperatorPublic(self.0.verifying_key().to_bytes())
    }

    /// The approval of `event`, a human decision, in the log whose engine key is `engine`, after the
    /// event at `after` whose hash is `after_hash` (0 and all zeros on an empty log).
    pub fn approve(
        &self,
        engine: &[u8; 32],
        after: u64,
        after_hash: Digest,
        event: &[u8],
    ) -> OperatorApproval {
        let signature = self
            .0
            .sign(&message(engine, after, &after_hash, event))
            .to_bytes();
        OperatorApproval {
            operator: self.public(),
            after,
            after_hash,
            signature: ApprovalSignature(signature),
        }
    }
}

impl fmt::Debug for OperatorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OperatorKey(public {})", self.public().to_hex())
    }
}

fn message(engine: &[u8; 32], after: u64, after_hash: &Digest, event: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(APPROVAL_TAG.len() + 72 + event.len());
    message.extend_from_slice(APPROVAL_TAG);
    message.extend_from_slice(engine);
    message.extend_from_slice(&after.to_be_bytes());
    message.extend_from_slice(after_hash.as_bytes());
    message.extend_from_slice(event);
    message
}

/// The operator's public key, as `operator.pub` holds it in hexadecimal.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperatorPublic([u8; 32]);

impl OperatorPublic {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        hex::from_hex::<32>(text.trim()).map(Self)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::to_hex(&self.0)
    }
}

impl fmt::Display for OperatorPublic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for OperatorPublic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OperatorPublic({})", self.to_hex())
    }
}

impl Serialize for OperatorPublic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for OperatorPublic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        hex::from_hex::<32>(&text)
            .map(Self)
            .ok_or_else(|| D::Error::custom("a public key is 64 hexadecimal characters"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ApprovalSignature([u8; APPROVAL_SIGNATURE_LEN]);

impl ApprovalSignature {
    pub fn from_bytes(bytes: [u8; APPROVAL_SIGNATURE_LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; APPROVAL_SIGNATURE_LEN] {
        &self.0
    }
}

impl fmt::Debug for ApprovalSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApprovalSignature({})", hex::to_hex(&self.0))
    }
}

impl Serialize for ApprovalSignature {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::to_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for ApprovalSignature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        hex::from_hex::<APPROVAL_SIGNATURE_LEN>(&text)
            .map(Self)
            .ok_or_else(|| D::Error::custom("a signature is 128 hexadecimal characters"))
    }
}

/// The operator's signature on one human decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorApproval {
    /// Who signed: checked against `operator.pub`, never taken from the log.
    pub operator: OperatorPublic,
    /// The last event the operator's command saw, and its hash.
    pub after: u64,
    pub after_hash: Digest,
    pub signature: ApprovalSignature,
}

impl OperatorApproval {
    /// Whether this is `operator`'s signature on `event` (encoded with [`Signed::None`]) in the log
    /// whose engine key is `engine`. Strict: it also refuses malleable signatures and weak keys.
    pub fn is_by(&self, operator: &OperatorPublic, engine: &[u8; 32], event: &[u8]) -> bool {
        if self.operator != *operator {
            return false;
        }
        let Ok(key) = VerifyingKey::from_bytes(&operator.0) else {
            return false;
        };
        key.verify_strict(
            &message(engine, self.after, &self.after_hash, event),
            &Signature::from_bytes(&self.signature.0),
        )
        .is_ok()
    }
}

/// Whether a decision carries the operator's signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signed {
    /// A decision that is not a human's: policy, the verifier, a gate's re-evaluation.
    None,
    /// A human decision written before schema version 13, which did not carry one. `hyalos log verify`
    /// lists them; the engine never writes one.
    Unrecorded,
    /// The operator signed it.
    Operator(OperatorApproval),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_approval_holds_only_for_its_operator_log_place_and_event() {
        let key = OperatorKey::from_seed([7; 32]);
        let engine = [1; 32];
        let after = Digest::from_bytes([2; 32]);
        let approval = key.approve(&engine, 4, after, b"event");
        let me = key.public();
        assert!(approval.is_by(&me, &engine, b"event"));
        assert!(!approval.is_by(&me, &engine, b"evenT"));
        assert!(!approval.is_by(&me, &[9; 32], b"event"));
        let other = OperatorKey::from_seed([8; 32]).public();
        assert!(!approval.is_by(&other, &engine, b"event"));
        let moved = OperatorApproval {
            after: 5,
            ..approval.clone()
        };
        assert!(!moved.is_by(&me, &engine, b"event"));
        let rehashed = OperatorApproval {
            after_hash: Digest::from_bytes([3; 32]),
            ..approval.clone()
        };
        assert!(!rehashed.is_by(&me, &engine, b"event"));
        // Another key claiming the operator's name does not make its signature the operator's.
        let forged = OperatorApproval {
            operator: me,
            ..OperatorKey::from_seed([8; 32]).approve(&engine, 4, after, b"event")
        };
        assert!(!forged.is_by(&me, &engine, b"event"));
    }

    #[test]
    fn signed_reads_and_writes_its_three_forms() {
        let approval =
            OperatorKey::from_seed([7; 32]).approve(&[1; 32], 0, Digest::from_bytes([0; 32]), b"x");
        for signed in [Signed::None, Signed::Unrecorded, Signed::Operator(approval)] {
            let json = serde_json::to_string(&signed).unwrap();
            assert_eq!(serde_json::from_str::<Signed>(&json).unwrap(), signed);
        }
        assert_eq!(serde_json::to_string(&Signed::None).unwrap(), "\"none\"");
        let extra = r#"{"operator":{"operator":"00","after":0,"after_hash":"00","signature":"00","more":1}}"#;
        assert!(serde_json::from_str::<Signed>(extra).is_err());
    }
}
