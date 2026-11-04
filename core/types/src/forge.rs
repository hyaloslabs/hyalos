//! A per-job forge token: what the engine mints from the GitHub App and hands the broker with the
//! manifest (ADR-0004, ADR-0044).
//!
//! It is a secret. It crosses only the private channel from the engine to the broker, never the log,
//! the sandbox or a message from the broker, and its `Debug` form does not show it.

use std::fmt;

use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};

use crate::Timestamp;

/// The longest token accepted. GitHub's installation tokens are about 40 characters; the room is for
/// a format change, not for arbitrary text.
pub const MAX_FORGE_TOKEN: usize = 255;

/// A token scoped to one job's repository and the forge permissions its actions need, and when the
/// forge will stop accepting it.
#[derive(Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeToken {
    token: Secret,
    /// When the forge stops accepting the token, whatever the engine does. The engine also revokes it
    /// when the job ends.
    pub expires_at: Timestamp,
}

impl ForgeToken {
    /// `None` unless `token` is 1 to [`MAX_FORGE_TOKEN`] printable ASCII characters with no space, so
    /// it can go in a header and can never carry a line break.
    pub fn new(token: &str, expires_at: Timestamp) -> Option<Self> {
        Secret::new(token).map(|token| Self { token, expires_at })
    }

    /// The token itself, for the one header that carries it.
    pub fn expose(&self) -> &str {
        &self.token.0
    }
}

impl fmt::Debug for ForgeToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ForgeToken(.., expires_at: {:?})", self.expires_at)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Secret(String);

impl Secret {
    fn new(text: &str) -> Option<Self> {
        let ok = !text.is_empty()
            && text.len() <= MAX_FORGE_TOKEN
            && text.bytes().all(|b| b.is_ascii_graphic());
        ok.then(|| Self(text.to_string()))
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Secret::new(&text).ok_or_else(|| D::Error::custom("not a forge token"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_plain_printable_ascii_and_never_shown() {
        let at = Timestamp::from_millis(5);
        let t = ForgeToken::new("ghs_abc123", at).unwrap();
        assert_eq!(t.expose(), "ghs_abc123");
        assert!(!format!("{t:?}").contains("ghs_abc123"));
        for bad in ["", "has space", "line\nbreak", "ünicode", &"a".repeat(256)] {
            assert!(ForgeToken::new(bad, at).is_none(), "{bad:?}");
        }
        assert!(ForgeToken::new(&"a".repeat(255), at).is_some());
    }

    #[test]
    fn a_token_is_checked_on_the_way_in() {
        let t = ForgeToken::new("ghs_x", Timestamp::from_millis(9)).unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"token":"ghs_x","expires_at":9}"#);
        assert_eq!(serde_json::from_str::<ForgeToken>(&json).unwrap(), t);
        for bad in [
            r#"{"token":"a b","expires_at":9}"#,
            r#"{"token":"","expires_at":9}"#,
            r#"{"token":"x","expires_at":9,"scope":"all"}"#,
            r#"{"token":"x"}"#,
        ] {
            assert!(serde_json::from_str::<ForgeToken>(bad).is_err(), "{bad}");
        }
    }
}
