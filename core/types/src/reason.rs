//! The answer a gate gives, and the short code that says why (ADR-0009, ADR-0020).

use std::fmt;

use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};

/// What a gate says about one request. There is no fourth value and no default: a gate that cannot
/// answer says `Unknown`, and unknown holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Allow,
    Deny,
    Unknown,
}

pub const MAX_REASON_LEN: usize = 64;

/// Why a gate answered as it did: a short code such as `egress.host_not_allowed`, of lowercase
/// letters, digits, `.`, `_` and `-`. A code cannot carry free text, so a reason can never smuggle a
/// secret, a prompt or a page of model output into the log (ADR-0003).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReasonCode(String);

impl ReasonCode {
    pub fn new(code: &str) -> Option<Self> {
        let valid = !code.is_empty()
            && code.len() <= MAX_REASON_LEN
            && code
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'));
        valid.then(|| Self(code.to_string()))
    }

    /// The gate did not answer within its deadline (ADR-0020).
    pub fn timed_out() -> Self {
        Self("gate.timed_out".to_string())
    }

    /// The gate could not be reached or failed (ADR-0020).
    pub fn unavailable() -> Self {
        Self("gate.unavailable".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReasonCode({})", self.0)
    }
}

impl Serialize for ReasonCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ReasonCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ReasonCode::new(&text).ok_or_else(|| {
            D::Error::custom(
                "a reason is 1 to 64 characters of lowercase letters, digits, '.', '_' and '-'",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_short_and_plain() {
        for ok in [
            "a",
            "egress.host_not_allowed",
            "gate.timed_out",
            "x-1",
            &"a".repeat(64),
        ] {
            assert!(ReasonCode::new(ok).is_some(), "{ok}");
        }
        for bad in [
            "",
            "Has Capitals",
            "has space",
            "sk-ant-ünicode",
            "line\nbreak",
            "semi;colon",
            "quote\"",
            &"a".repeat(65),
        ] {
            assert!(ReasonCode::new(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_built_in_codes_are_valid_codes() {
        for r in [ReasonCode::timed_out(), ReasonCode::unavailable()] {
            assert_eq!(ReasonCode::new(r.as_str()), Some(r));
        }
    }

    #[test]
    fn a_code_is_a_json_string_and_is_checked_on_the_way_in() {
        let r = ReasonCode::new("gate.timed_out").unwrap();
        assert_eq!(serde_json::to_string(&r).unwrap(), "\"gate.timed_out\"");
        assert_eq!(
            serde_json::from_str::<ReasonCode>("\"gate.timed_out\"").unwrap(),
            r
        );
        assert!(serde_json::from_str::<ReasonCode>("\"Not A Code\"").is_err());
        assert!(serde_json::from_str::<ReasonCode>("5").is_err());
    }

    #[test]
    fn answers_are_lowercase_words() {
        assert_eq!(
            serde_json::to_string(&Answer::Unknown).unwrap(),
            "\"unknown\""
        );
        assert!(serde_json::from_str::<Answer>("\"maybe\"").is_err());
    }
}
