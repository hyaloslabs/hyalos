//! The engine's log-signing key and its public half (ADR-0018).
//!
//! These wrap the signature library so no other crate needs it. Generating a key needs real entropy,
//! which this crate may not use: the binary draws a 32-byte seed from the operating system and passes
//! it to [`LogKey::from_seed`]. Signing itself is deterministic.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::format::{self, Hash, SIGNATURE_LEN};

/// The engine's secret signing key. The broker never has one (ADR-0002).
pub struct LogKey(SigningKey);

impl LogKey {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.verifying_key().to_bytes())
    }

    pub(crate) fn sign(&self, hash: &Hash) -> [u8; SIGNATURE_LEN] {
        self.0.sign(&format::signing_message(hash)).to_bytes()
    }
}

// A derived Debug would be safe today and one field away from printing the secret.
impl std::fmt::Debug for LogKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LogKey(public {})", self.public().to_hex())
    }
}

/// The public key a log is verified against. The operator supplies it from outside the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        format::from_hex::<32>(text.trim()).map(Self)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        format::to_hex(&self.0)
    }

    /// Strict verification: it also refuses malleable signatures and weak keys.
    pub(crate) fn verify(&self, hash: &Hash, signature: &[u8]) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        let Ok(bytes) = <[u8; SIGNATURE_LEN]>::try_from(signature) else {
            return false;
        };
        key.verify_strict(
            &format::signing_message(hash),
            &Signature::from_bytes(&bytes),
        )
        .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies_only_for_its_key_and_hash() {
        let key = LogKey::from_seed([1; 32]);
        let other = LogKey::from_seed([2; 32]);
        let hash = [9u8; 32];
        let sig = key.sign(&hash);
        assert!(key.public().verify(&hash, &sig));
        assert!(!other.public().verify(&hash, &sig));
        assert!(!key.public().verify(&[8u8; 32], &sig));
        assert!(!key.public().verify(&hash, &sig[..63]));
    }

    #[test]
    fn signing_is_deterministic() {
        let key = LogKey::from_seed([1; 32]);
        assert_eq!(key.sign(&[3; 32]), key.sign(&[3; 32]));
    }

    #[test]
    fn debug_shows_the_public_key_and_not_the_seed() {
        let text = format!("{:?}", LogKey::from_seed([0xAB; 32]));
        assert!(text.contains(&LogKey::from_seed([0xAB; 32]).public().to_hex()));
        assert!(!text.contains("abababab"));
    }

    #[test]
    fn a_public_key_parses_from_hex() {
        let public = LogKey::from_seed([5; 32]).public();
        assert_eq!(
            PublicKey::from_hex(&format!("{}\n", public.to_hex())),
            Some(public)
        );
        assert_eq!(PublicKey::from_hex("not hex"), None);
    }
}
