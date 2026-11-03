//! The wall clock, for the processes that are not deterministic (ADR-0013 keeps it out of the
//! deterministic crates, which are handed a `Clock`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyalos_types::{Clock, Timestamp};

/// The system's time in milliseconds since 1970.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        from_since_epoch(SystemTime::now().duration_since(UNIX_EPOCH).ok())
    }
}

/// The time read as `since_epoch`, or `None` for a clock set before 1970. That, and a time past what
/// a `u64` of milliseconds holds, reads as the far future, so every deadline has passed. It fails
/// closed: a broken clock refuses jobs and never extends them.
fn from_since_epoch(since_epoch: Option<Duration>) -> Timestamp {
    let millis = since_epoch
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(u64::MAX);
    Timestamp::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_that_cannot_say_reads_as_the_end_of_time() {
        assert_eq!(
            from_since_epoch(Some(Duration::from_millis(1_500))),
            Timestamp::from_millis(1_500)
        );
        assert_eq!(from_since_epoch(None), Timestamp::from_millis(u64::MAX));
        assert_eq!(
            from_since_epoch(Some(Duration::MAX)),
            Timestamp::from_millis(u64::MAX)
        );
    }

    #[test]
    fn the_system_clock_is_after_2020_and_does_not_go_back() {
        let a = SystemClock.now();
        let b = SystemClock.now();
        assert!(a.as_millis() > 1_577_836_800_000);
        assert!(b >= a);
    }
}
