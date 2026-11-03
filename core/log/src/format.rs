//! The bytes that are hashed and signed (ADR-0018).
//!
//! Every field is hashed exactly as it is stored, with a length in front of anything variable, so the
//! verifier recomputes a hash from the stored row and never has to re-serialize a payload. A change to
//! any of these functions changes every hash, so it needs a new format version.

use hyalos_types::Origin;
use sha2::{Digest, Sha256};

pub const HASH_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;

pub type Hash = [u8; HASH_LEN];

const GENESIS_TAG: &[u8] = b"hyalos.log.genesis.v1\0";
const EVENT_TAG: &[u8] = b"hyalos.log.event.v1\0";
const SIGNATURE_TAG: &[u8] = b"hyalos.log.sig.v1\0";

/// The `prev_hash` of event 1. It binds the chain to one public key, so a log verified with the wrong
/// key fails at its first event.
pub fn genesis_hash(public_key: &[u8; 32]) -> Hash {
    let mut h = Sha256::new();
    h.update(GENESIS_TAG);
    h.update(public_key);
    h.finalize().into()
}

/// One event as it is stored, minus the hash and signature.
pub struct Fields<'a> {
    pub seq: u64,
    pub at_millis: u64,
    pub origin: Origin,
    pub kind: &'a str,
    pub schema_version: u32,
    pub payload: &'a [u8],
    pub prev_hash: &'a Hash,
}

pub fn event_hash(f: &Fields<'_>) -> Hash {
    let origin = f.origin.as_str().as_bytes();
    let mut h = Sha256::new();
    h.update(EVENT_TAG);
    h.update(f.seq.to_be_bytes());
    h.update(f.at_millis.to_be_bytes());
    h.update((origin.len() as u64).to_be_bytes());
    h.update(origin);
    h.update((f.kind.len() as u64).to_be_bytes());
    h.update(f.kind.as_bytes());
    h.update(f.schema_version.to_be_bytes());
    h.update((f.payload.len() as u64).to_be_bytes());
    h.update(f.payload);
    h.update(f.prev_hash);
    h.finalize().into()
}

/// What the engine signs: the event hash under its own tag, so a signature made here cannot be
/// replayed as a signature over anything else.
pub fn signing_message(hash: &Hash) -> Vec<u8> {
    let mut m = Vec::with_capacity(SIGNATURE_TAG.len() + HASH_LEN);
    m.extend_from_slice(SIGNATURE_TAG);
    m.extend_from_slice(hash);
    m
}

pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}

/// Parses exactly `N` bytes of lowercase or uppercase hex.
pub fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.as_bytes();
    if text.len() != N * 2 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
        out[i] = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields<'a>(payload: &'a [u8], prev: &'a Hash) -> Fields<'a> {
        Fields {
            seq: 1,
            at_millis: 1_000,
            origin: Origin::Engine,
            kind: "task.created",
            schema_version: 1,
            payload,
            prev_hash: prev,
        }
    }

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(to_hex(&bytes), "000fa5ff");
        assert_eq!(from_hex::<4>("000fa5ff"), Some(bytes));
        assert_eq!(from_hex::<4>("000FA5FF"), Some(bytes));
        assert_eq!(from_hex::<4>("000fa5"), None);
        assert_eq!(from_hex::<4>("000fa5fg"), None);
        assert_eq!(from_hex::<4>("000fa5ff00"), None);
    }

    #[test]
    fn the_hash_is_pinned() {
        // A change here means every existing log stops verifying, so it must be deliberate.
        let prev = [7u8; HASH_LEN];
        assert_eq!(
            to_hex(&event_hash(&fields(b"{}", &prev))),
            "21d3060c578a5bbef5bedceb3ce79427efb17249dfd93615a4ad8214edc4f825"
        );
    }

    #[test]
    fn every_field_changes_the_hash() {
        let prev = [7u8; HASH_LEN];
        let base = event_hash(&fields(b"{}", &prev));
        let other_prev = [8u8; HASH_LEN];
        let variants = [
            Fields {
                seq: 2,
                ..fields(b"{}", &prev)
            },
            Fields {
                at_millis: 1_001,
                ..fields(b"{}", &prev)
            },
            Fields {
                origin: Origin::Broker,
                ..fields(b"{}", &prev)
            },
            Fields {
                kind: "task.created2",
                ..fields(b"{}", &prev)
            },
            Fields {
                schema_version: 2,
                ..fields(b"{}", &prev)
            },
            fields(b"{ }", &prev),
            fields(b"{}", &other_prev),
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(event_hash(v), base, "variant {i} did not change the hash");
        }
    }

    #[test]
    fn moving_bytes_between_kind_and_payload_changes_the_hash() {
        // Without length prefixes these two would hash the same bytes.
        let prev = [0u8; HASH_LEN];
        let a = Fields {
            kind: "ab",
            ..fields(b"c", &prev)
        };
        let b = Fields {
            kind: "a",
            ..fields(b"bc", &prev)
        };
        assert_ne!(event_hash(&a), event_hash(&b));
    }

    #[test]
    fn the_genesis_depends_on_the_key() {
        assert_ne!(genesis_hash(&[1; 32]), genesis_hash(&[2; 32]));
    }
}
