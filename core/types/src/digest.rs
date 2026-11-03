//! A SHA-256 digest, as it appears in events (ADR-0003).
//!
//! Events hold digests and references instead of content, so the log never carries raw model text or
//! raw forge content. A `Digest` is a value; computing one is the caller's job.

use std::fmt;

use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};

use crate::hex;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn parse(text: &str) -> Option<Self> {
        hex::from_hex::<32>(text).map(Self)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::to_hex(&self.0))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Digest::parse(&text)
            .ok_or_else(|| D::Error::custom("a digest is 64 hexadecimal characters"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_prints_and_parses_as_hex() {
        let d = Digest::from_bytes([0xAB; 32]);
        assert_eq!(d.to_string(), "ab".repeat(32));
        assert_eq!(Digest::parse(&d.to_string()), Some(d));
        assert_eq!(Digest::parse("abab"), None);
    }

    #[test]
    fn a_digest_is_a_json_string() {
        let d = Digest::from_bytes([1; 32]);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, format!("\"{}\"", "01".repeat(32)));
        assert_eq!(serde_json::from_str::<Digest>(&json).unwrap(), d);
        assert!(serde_json::from_str::<Digest>("\"nope\"").is_err());
        assert!(serde_json::from_str::<Digest>("12").is_err());
    }
}
