//! Which event schema versions a build reads, and how old ones become current (ADR-0023).
//!
//! The log keeps every event exactly as it was written, so an old event is upcast in memory when it is
//! read. Steps are pure functions of one payload.

use std::fmt;

pub use serde_json::Value;

use crate::event::{Event, EventError};

/// Turns the payload of one kind at version `from` into its payload at `from + 1`.
pub type UpcastFn = fn(Value) -> Result<Value, String>;

#[derive(Clone, Copy)]
pub struct Upcast {
    pub kind: &'static str,
    /// The version this step reads. It produces `from + 1`.
    pub from: u32,
    pub step: UpcastFn,
}

/// The current event schema version and the steps from every older one.
pub struct Schema {
    pub current: u32,
    pub steps: &'static [Upcast],
}

impl fmt::Debug for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Schema")
            .field("current", &self.current)
            .field("steps", &self.steps.len())
            .finish()
    }
}

impl Schema {
    /// Checks the table itself, so a mistake shows when the engine starts and not when an old event
    /// happens to be read.
    pub fn validate(&self) -> Result<(), String> {
        if self.current == 0 {
            return Err("the current schema version is 0; versions start at 1".into());
        }
        for (i, s) in self.steps.iter().enumerate() {
            if !Event::KINDS.contains(&s.kind) {
                return Err(format!(
                    "an upcast step names unknown event kind `{}`",
                    s.kind
                ));
            }
            if s.from == 0 || s.from >= self.current {
                return Err(format!(
                    "the upcast step for `{}` from version {} is outside 1..{}",
                    s.kind, s.from, self.current
                ));
            }
            if self.steps[..i]
                .iter()
                .any(|o| o.kind == s.kind && o.from == s.from)
            {
                return Err(format!(
                    "two upcast steps for `{}` from version {}",
                    s.kind, s.from
                ));
            }
        }
        Ok(())
    }

    /// Brings `payload`, written at `version`, up to the current version. A kind with no step at some
    /// version did not change there.
    pub fn upcast(&self, kind: &str, version: u32, payload: Value) -> Result<Value, EventError> {
        self.check_version(version)?;
        let mut value = payload;
        for from in version..self.current {
            if let Some(s) = self.steps.iter().find(|s| s.kind == kind && s.from == from) {
                value = (s.step)(value).map_err(|reason| EventError::Upcast {
                    kind: kind.to_string(),
                    from,
                    reason,
                })?;
            }
        }
        Ok(value)
    }

    pub(crate) fn check_version(&self, version: u32) -> Result<(), EventError> {
        if version == 0 || version > self.current {
            return Err(EventError::UnsupportedVersion {
                found: version,
                current: self.current,
            });
        }
        Ok(())
    }
}
