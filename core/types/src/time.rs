//! Time as a value, and the one way the engine is allowed to ask for it (ADR-0013).
//!
//! The deterministic crates may not read the system clock. They are handed a [`Clock`]. The binary
//! supplies one backed by the real clock; the simulator supplies one it advances by hand.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch. In JSON it is the plain number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// Adds `delta`, stopping at the largest representable time instead of wrapping.
    pub fn saturating_add(self, delta: Duration) -> Self {
        let millis = u64::try_from(delta.as_millis()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(millis))
    }
}

/// A source of the current time. Implementations must never go backwards.
pub trait Clock {
    fn now(&self) -> Timestamp;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_order_by_millis() {
        assert!(Timestamp::from_millis(1) < Timestamp::from_millis(2));
        assert_eq!(Timestamp::from_millis(7).as_millis(), 7);
    }

    #[test]
    fn adding_a_duration_moves_forward_in_millis() {
        let t = Timestamp::from_millis(1_000).saturating_add(Duration::from_secs(2));
        assert_eq!(t, Timestamp::from_millis(3_000));
    }

    #[test]
    fn adding_stops_at_the_largest_time() {
        let t = Timestamp::from_millis(u64::MAX - 1).saturating_add(Duration::from_secs(5));
        assert_eq!(t, Timestamp::from_millis(u64::MAX));
    }
}
