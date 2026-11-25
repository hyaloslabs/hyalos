//! A stand-in for `web-time` 1.1 in the spike's Monty guest. On `wasm32-unknown-unknown`, `web-time`
//! reads JavaScript's `performance.now()`, an import no hyalOS profile allows. This clock never
//! moves: every `Instant` is the same, so every elapsed time is zero and a run never depends on
//! when it ran.

use core::ops::{Add, AddAssign, Sub, SubAssign};
pub use core::time::Duration;

/// An instant on a clock that never moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(Duration);

impl Instant {
    pub fn now() -> Self {
        Instant(Duration::ZERO)
    }
    pub fn elapsed(&self) -> Duration {
        Self::now().saturating_duration_since(*self)
    }
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.saturating_duration_since(earlier)
    }
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }
    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
    pub fn checked_add(&self, d: Duration) -> Option<Instant> {
        self.0.checked_add(d).map(Instant)
    }
    pub fn checked_sub(&self, d: Duration) -> Option<Instant> {
        self.0.checked_sub(d).map(Instant)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d)
    }
}
impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, d: Duration) {
        self.0 += d;
    }
}
impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, d: Duration) -> Instant {
        Instant(self.0.saturating_sub(d))
    }
}
impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, d: Duration) {
        self.0 = self.0.saturating_sub(d);
    }
}
impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.saturating_duration_since(other)
    }
}
