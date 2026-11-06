//! The name a job's substrate is recorded under (ADR-0011, ADR-0043), and what it could attest to
//! (ADR-0057).

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

/// What the substrate could show about the sandbox a job ran in, as evidence an auditor can check
/// without trusting the engine (ADR-0011, ADR-0057). No driver offers evidence yet, so each job
/// records which reason applies.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Attestation {
    /// The substrate offers no evidence, for the reason the code names.
    None(crate::ReasonCode),
    /// A `job.started` written before schema version 9, which did not say.
    Unrecorded,
}

impl Attestation {
    /// None, because the substrate has no hardware root to sign what it started: true of a
    /// container, gVisor and a Firecracker microVM.
    pub fn no_hardware_root() -> Self {
        Attestation::None(crate::ReasonCode::no_hardware_root())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attestation_is_a_reason_or_unrecorded_and_nothing_else() {
        let none = Attestation::no_hardware_root();
        assert_eq!(
            serde_json::to_string(&none).unwrap(),
            r#"{"none":"attestation.no_hardware_root"}"#
        );
        assert_eq!(
            serde_json::to_string(&Attestation::Unrecorded).unwrap(),
            r#""unrecorded""#
        );
        for bad in [
            r#"{"none":"Free text here"}"#,
            r#"{"evidence":"00"}"#,
            r#"{"none":"attestation.x","extra":1}"#,
            r#""none""#,
        ] {
            assert!(serde_json::from_str::<Attestation>(bad).is_err(), "{bad}");
        }
    }

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
