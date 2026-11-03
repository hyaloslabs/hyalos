//! The state a snapshot carries (ADR-0026).
//!
//! Compaction replaces events with a signed snapshot of the state they led to. The log signs the bytes
//! without reading them; this is the layout of those bytes. Like events they are typed values,
//! digests and references, never secrets, and decoding refuses unknown fields and unknown versions.
//!
//! The image mirrors the engine's state field for field. It is a separate type so the engine's state
//! can change shape without its stored form changing silently: when the image changes, [`STATE_VERSION`]
//! is bumped and a step is added, as for events (ADR-0023).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    Digest, Gate, HoldId, HoldReason, JobId, JobOutcome, ReasonCode, Resolution, Stakes, TaskId,
    Value,
};

/// The layout [`StateImage`] is written in today.
///
/// Version 2 added the archives that were pruned (ADR-0026, S1.14). A version 1 image has none.
pub const STATE_VERSION: u32 = 2;

/// Every task, job and hold as of `last_seq`. Lists are in id order, so one state has one encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateImage {
    /// The last event the state covers, which is the event the snapshot stands for.
    pub last_seq: u64,
    /// The stamp of that event, in milliseconds, if there was one.
    pub last_at: Option<u64>,
    pub tasks: Vec<TaskImage>,
    pub jobs: Vec<JobImage>,
    pub holds: Vec<HoldImage>,
    /// Every archive pruned so far, in the order it was recorded. It lives in the snapshot so that
    /// compacting again does not take the record out of the live log.
    pub pruned: Vec<PrunedImage>,
}

/// An `archive.pruned` event, kept (ADR-0026).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrunedImage {
    /// The sequence number of the event that recorded it.
    pub seq: u64,
    pub at: u64,
    /// The last event the archive held, and its hash.
    pub last_seq: u64,
    pub last_hash: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskImage {
    pub id: TaskId,
    pub created_at: u64,
    pub request: Digest,
    pub completed_at: Option<u64>,
    /// In id order.
    pub open_holds: Vec<HoldId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobImage {
    pub id: JobId,
    pub task: TaskId,
    pub started_at: u64,
    pub ended: Option<JobEndImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobEndImage {
    pub at: u64,
    pub outcome: JobOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldImage {
    pub id: HoldId,
    pub task: TaskId,
    pub gate: Gate,
    pub inputs: Digest,
    pub reason: HoldReason,
    pub stakes: Stakes,
    pub opened_at: u64,
    pub resolved: Option<ResolvedImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedImage {
    pub at: u64,
    pub by: Resolution,
    pub reason: ReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Written by a build this one does not know: a newer one, or no version at all. Never skipped.
    UnsupportedVersion {
        found: u32,
        current: u32,
    },
    Encode(String),
    Decode(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::UnsupportedVersion { found, current } => write!(
                f,
                "the snapshot's state is version {found} but this Hyalos reads up to version {current}: it was written by a newer Hyalos (or is damaged). Upgrade to read it"
            ),
            ImageError::Encode(e) => write!(f, "cannot encode the state: {e}"),
            ImageError::Decode(e) => write!(f, "cannot read the snapshot's state: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl StateImage {
    /// The bytes a snapshot stores, at [`STATE_VERSION`].
    pub fn encode(&self) -> Result<Vec<u8>, ImageError> {
        serde_json::to_vec(self).map_err(|e| ImageError::Encode(e.to_string()))
    }

    /// Reads bytes written at `version`, bringing an older one up to date.
    pub fn decode(version: u32, bytes: &[u8]) -> Result<Self, ImageError> {
        if version == 0 || version > STATE_VERSION {
            return Err(ImageError::UnsupportedVersion {
                found: version,
                current: STATE_VERSION,
            });
        }
        if version == STATE_VERSION {
            return serde_json::from_slice(bytes).map_err(|e| ImageError::Decode(e.to_string()));
        }
        let mut value: Value =
            serde_json::from_slice(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
        // 1 -> 2: nothing had been pruned.
        if version == 1 {
            value
                .as_object_mut()
                .ok_or_else(|| ImageError::Decode("the state is not an object".into()))?
                .insert("pruned".into(), Value::Array(Vec::new()));
        }
        serde_json::from_value(value).map_err(|e| ImageError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(byte: u8) -> Digest {
        Digest::from_bytes([byte; 32])
    }

    fn sample() -> StateImage {
        StateImage {
            last_seq: 9,
            last_at: Some(4_000),
            tasks: vec![TaskImage {
                id: TaskId::from_seq(1),
                created_at: 1_000,
                request: d(1),
                completed_at: None,
                open_holds: vec![HoldId::from_seq(5)],
            }],
            jobs: vec![JobImage {
                id: JobId::from_seq(2),
                task: TaskId::from_seq(1),
                started_at: 2_000,
                ended: Some(JobEndImage {
                    at: 3_000,
                    outcome: JobOutcome::Held,
                }),
            }],
            holds: vec![HoldImage {
                id: HoldId::from_seq(5),
                task: TaskId::from_seq(1),
                gate: Gate::Egress,
                inputs: d(3),
                reason: HoldReason::Unknown,
                stakes: Stakes::irreversible(),
                opened_at: 3_000,
                resolved: None,
            }],
            pruned: vec![PrunedImage {
                seq: 11,
                at: 4_500,
                last_seq: 8,
                last_hash: d(9),
            }],
        }
    }

    #[test]
    fn an_image_survives_encoding() {
        let image = sample();
        let bytes = image.encode().unwrap();
        assert_eq!(StateImage::decode(STATE_VERSION, &bytes).unwrap(), image);
        assert_eq!(image.encode().unwrap(), bytes, "one state, one encoding");
    }

    #[test]
    fn a_version_this_build_does_not_read_is_refused_not_skipped() {
        let bytes = sample().encode().unwrap();
        for version in [0, STATE_VERSION + 1, u32::MAX] {
            assert_eq!(
                StateImage::decode(version, &bytes),
                Err(ImageError::UnsupportedVersion {
                    found: version,
                    current: STATE_VERSION
                })
            );
        }
        let text = ImageError::UnsupportedVersion {
            found: 9,
            current: 1,
        }
        .to_string();
        assert!(text.contains("Upgrade"), "{text}");
    }

    #[test]
    fn unknown_fields_and_malformed_bytes_are_refused() {
        let mut value: Value = serde_json::from_slice(&sample().encode().unwrap()).unwrap();
        value["surprise"] = true.into();
        let with_extra = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            StateImage::decode(STATE_VERSION, &with_extra),
            Err(ImageError::Decode(_))
        ));
        assert!(StateImage::decode(STATE_VERSION, b"not json").is_err());
        assert!(StateImage::decode(STATE_VERSION, b"{}").is_err());
    }

    use crate::Value;
}
