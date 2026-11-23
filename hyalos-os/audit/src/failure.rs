//! Why an audit did not pass. A failed check on readable evidence is a refusal. Evidence that could
//! not be read or reached is inconclusive. Neither is ever a pass.

use std::{fmt, ops::Deref};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Refused,
    Inconclusive,
}

/// One failure, with the first check it broke. The text holds no secret and no task data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

impl Failure {
    pub fn refused(message: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Refused,
            message: message.into(),
        }
    }

    pub fn inconclusive(message: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Inconclusive,
            message: message.into(),
        }
    }
}

/// An error with no stated kind is a refusal, so a new one can never read as a pass.
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::refused(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::refused(message)
    }
}

impl Deref for Failure {
    type Target = str;

    fn deref(&self) -> &str {
        &self.message
    }
}

impl PartialEq<&str> for Failure {
    fn eq(&self, other: &&str) -> bool {
        self.message == *other
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
