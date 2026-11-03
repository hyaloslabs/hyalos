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
const ANCHOR_TAG: &[u8] = b"hyalos.log.anchor.v1\0";
const ANCHOR_GENESIS_TAG: &[u8] = b"hyalos.log.anchor.genesis.v1\0";
const ANCHOR_SIGNATURE_TAG: &[u8] = b"hyalos.log.anchor.sig.v1\0";
const STATE_TAG: &[u8] = b"hyalos.log.state.v1\0";

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

/// The `prev_anchor` of a log's first snapshot. Like [`genesis_hash`] it binds the chain of snapshots
/// to one public key.
pub fn first_anchor_prev(public_key: &[u8; 32]) -> Hash {
    let mut h = Sha256::new();
    h.update(ANCHOR_GENESIS_TAG);
    h.update(public_key);
    h.finalize().into()
}

/// The digest a snapshot commits to for its state bytes. The log never reads the bytes; it only
/// guarantees they are the ones that were signed.
pub fn state_digest(state: &[u8]) -> Hash {
    let mut h = Sha256::new();
    h.update(STATE_TAG);
    h.update((state.len() as u64).to_be_bytes());
    h.update(state);
    h.finalize().into()
}

/// One snapshot as it is stored, minus its own hash and signature (ADR-0026).
pub struct AnchorFields<'a> {
    /// The last event the snapshot covers.
    pub seq: u64,
    pub at_millis: u64,
    /// The hash of that event, which commits to every event before it.
    pub head_hash: &'a Hash,
    pub state_version: u32,
    pub state_digest: &'a Hash,
    /// The hash of the snapshot before this one, so none can be removed unseen.
    pub prev_anchor: &'a Hash,
}

pub fn anchor_hash(f: &AnchorFields<'_>) -> Hash {
    let mut h = Sha256::new();
    h.update(ANCHOR_TAG);
    h.update(f.seq.to_be_bytes());
    h.update(f.at_millis.to_be_bytes());
    h.update(f.head_hash);
    h.update(f.state_version.to_be_bytes());
    h.update(f.state_digest);
    h.update(f.prev_anchor);
    h.finalize().into()
}

/// What the engine signs for a snapshot: under its own tag, so it cannot be replayed as an event's
/// signature or the other way round.
pub fn anchor_signing_message(hash: &Hash) -> Vec<u8> {
    let mut m = Vec::with_capacity(ANCHOR_SIGNATURE_TAG.len() + HASH_LEN);
    m.extend_from_slice(ANCHOR_SIGNATURE_TAG);
    m.extend_from_slice(hash);
    m
}

pub use hyalos_types::hex::{from_hex, to_hex};

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
    fn the_anchor_hash_is_pinned() {
        // A change here means every compacted log stops verifying, so it must be deliberate.
        let (head, digest, prev) = ([1u8; HASH_LEN], [2u8; HASH_LEN], [3u8; HASH_LEN]);
        let fields = AnchorFields {
            seq: 40,
            at_millis: 5_000,
            head_hash: &head,
            state_version: 1,
            state_digest: &digest,
            prev_anchor: &prev,
        };
        assert_eq!(
            to_hex(&anchor_hash(&fields)),
            "3579fa9a9534ae9c4630a31068f7ba4b0d60c3b1b6cdb42439813d0dd6943bf5"
        );
    }

    #[test]
    fn every_field_changes_the_anchor_hash() {
        let (head, digest, prev) = ([1u8; HASH_LEN], [2u8; HASH_LEN], [3u8; HASH_LEN]);
        let other = [9u8; HASH_LEN];
        let base = AnchorFields {
            seq: 40,
            at_millis: 5_000,
            head_hash: &head,
            state_version: 1,
            state_digest: &digest,
            prev_anchor: &prev,
        };
        let reference = anchor_hash(&base);
        let variants = [
            AnchorFields { seq: 41, ..base },
            AnchorFields {
                at_millis: 5_001,
                ..base
            },
            AnchorFields {
                head_hash: &other,
                ..base
            },
            AnchorFields {
                state_version: 2,
                ..base
            },
            AnchorFields {
                state_digest: &other,
                ..base
            },
            AnchorFields {
                prev_anchor: &other,
                ..base
            },
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(
                anchor_hash(v),
                reference,
                "variant {i} did not change the hash"
            );
        }
    }

    #[test]
    fn the_state_digest_covers_the_length_and_the_bytes() {
        assert_ne!(state_digest(b"ab"), state_digest(b"ac"));
        assert_ne!(state_digest(b""), state_digest(b"\0"));
        assert_eq!(state_digest(b"x"), state_digest(b"x"));
    }

    #[test]
    fn anchors_and_events_sign_under_different_tags() {
        let hash = [4u8; HASH_LEN];
        assert_ne!(signing_message(&hash), anchor_signing_message(&hash));
        assert_ne!(genesis_hash(&[1; 32]), first_anchor_prev(&[1; 32]));
    }

    #[test]
    fn the_genesis_depends_on_the_key() {
        assert_ne!(genesis_hash(&[1; 32]), genesis_hash(&[2; 32]));
    }
}
