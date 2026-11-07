//! The driving agent's credential (ADR-0010, ADR-0068): what the log records about one.
//!
//! The credential itself is a secret the operator gives the agent once. The log holds only its
//! SHA-256 (a [`crate::Digest`]), a name for the operator to recognise it by, and who issued or
//! revoked it, so the log and every backup of it can be read without exposing it.

use std::fmt;

use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};

pub const MAX_AGENT_NAME_LEN: usize = 64;

/// What the operator calls an agent's credential, such as `claude-code`: 1 to 64 lowercase letters,
/// digits, `.`, `_` and `-`. Like a [`crate::ReasonCode`] it cannot carry free text.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentName(String);

impl AgentName {
    pub fn new(name: &str) -> Option<Self> {
        let valid = !name.is_empty()
            && name.len() <= MAX_AGENT_NAME_LEN
            && name
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'));
        valid.then(|| Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AgentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AgentName({})", self.0)
    }
}

impl Serialize for AgentName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AgentName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        AgentName::new(&text).ok_or_else(|| {
            D::Error::custom(
                "an agent name is 1 to 64 characters of lowercase letters, digits, '.', '_' and '-'",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_short_code_and_reads_back_only_if_valid() {
        assert!(AgentName::new("claude-code").is_some());
        for bad in ["", "Claude", "a b", "a/b", &"x".repeat(65)] {
            assert!(AgentName::new(bad).is_none(), "{bad}");
        }
        let name: AgentName = serde_json::from_str(r#""hermes.1""#).unwrap();
        assert_eq!(serde_json::to_string(&name).unwrap(), r#""hermes.1""#);
        assert!(serde_json::from_str::<AgentName>(r#""Hermes""#).is_err());
    }
}
