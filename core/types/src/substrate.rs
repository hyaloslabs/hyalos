//! The name a job's substrate is recorded under (ADR-0011, ADR-0043).

use std::fmt;

use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};

pub const MAX_SUBSTRATE_NAME_LEN: usize = 64;

/// Which substrate ran a job, such as `container.podman`: the driver, then what it drove, of lowercase
/// letters, digits, `.`, `_` and `-`. Like a [`crate::ReasonCode`] it cannot carry free text, so the
/// path or version of a runtime never reaches the log.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubstrateName(String);

impl SubstrateName {
    pub fn new(name: &str) -> Option<Self> {
        let valid = !name.is_empty()
            && name.len() <= MAX_SUBSTRATE_NAME_LEN
            && name
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'));
        valid.then(|| Self(name.to_string()))
    }

    /// What a `job.started` written before schema version 6 reads as: the substrate was not recorded.
    pub fn unrecorded() -> Self {
        Self("unrecorded".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SubstrateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for SubstrateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SubstrateName({})", self.0)
    }
}

impl Serialize for SubstrateName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SubstrateName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        SubstrateName::new(&text).ok_or_else(|| {
            D::Error::custom(
                "a substrate name is 1 to 64 characters of lowercase letters, digits, '.', '_' and '-'",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_short_and_plain() {
        for ok in [
            "container.podman",
            "container.docker",
            "unrecorded",
            "local",
        ] {
            assert!(SubstrateName::new(ok).is_some(), "{ok}");
        }
        for bad in [
            "",
            "Container",
            "/usr/bin/podman",
            "podman 5.2",
            &"a".repeat(65),
        ] {
            assert!(SubstrateName::new(bad).is_none(), "{bad:?}");
        }
        assert_eq!(
            SubstrateName::new(SubstrateName::unrecorded().as_str()),
            Some(SubstrateName::unrecorded())
        );
    }

    #[test]
    fn a_name_is_a_json_string_and_is_checked_on_the_way_in() {
        let n = SubstrateName::new("container.docker").unwrap();
        assert_eq!(serde_json::to_string(&n).unwrap(), "\"container.docker\"");
        assert_eq!(
            serde_json::from_str::<SubstrateName>("\"container.docker\"").unwrap(),
            n
        );
        assert!(serde_json::from_str::<SubstrateName>("\"/usr/bin/docker\"").is_err());
    }
}
