//! Deterministic simulation harness (ADR-0013).
//!
//! A [`Sim`] is a pure function of its seed. It owns a fake clock that only moves when the test moves
//! it, and hands out one independent random stream per boundary (substrate, forge, model provider,
//! broker). Every fake draws from its own stream, so a failing run replays from the seed alone, and
//! adding a draw to one boundary never changes what another sees.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use hyalos_types::{Clock, SeededRng, Timestamp};

/// Where every simulated run starts, so traces do not depend on when the test was run.
pub const START: Timestamp = Timestamp::from_millis(1_800_000_000_000);

/// A clock that moves only when told to. Clones share the same time.
#[derive(Debug, Clone)]
pub struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    pub fn new(start: Timestamp) -> Self {
        Self(Arc::new(AtomicU64::new(start.as_millis())))
    }

    /// Moves time forward by `delta`. Time never goes backwards, and advances from several threads
    /// add up: the update is one atomic step, not a read followed by a store.
    pub fn advance(&self, delta: Duration) {
        let millis = u64::try_from(delta.as_millis()).unwrap_or(u64::MAX);
        self.0.update(Ordering::SeqCst, Ordering::SeqCst, |t| {
            t.saturating_add(millis)
        });
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// One simulated world: a seed, a fake clock and the streams derived from the seed.
#[derive(Debug, Clone)]
pub struct Sim {
    rng: SeededRng,
    clock: FakeClock,
}

impl Sim {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: SeededRng::new(seed),
            clock: FakeClock::new(START),
        }
    }

    pub fn seed(&self) -> u64 {
        self.rng.seed()
    }

    /// A handle to the simulated clock.
    pub fn clock(&self) -> FakeClock {
        self.clock.clone()
    }

    /// The random stream for one boundary, such as `"substrate"` or `"model"`. The same seed and name
    /// always give the same stream.
    pub fn stream(&self, name: &str) -> SeededRng {
        self.rng.fork(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_starts_at_a_fixed_time_and_only_moves_when_advanced() {
        let sim = Sim::new(1);
        let clock = sim.clock();
        assert_eq!(clock.now(), START);
        assert_eq!(clock.now(), START);
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.now().as_millis(), START.as_millis() + 250);
    }

    #[test]
    fn concurrent_advances_are_never_lost() {
        let sim = Sim::new(1);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let clock = sim.clock();
                std::thread::spawn(move || {
                    for _ in 0..2_000 {
                        clock.advance(Duration::from_millis(1));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(sim.clock().now().as_millis(), START.as_millis() + 16_000);
    }

    #[test]
    fn clock_handles_share_one_time() {
        let sim = Sim::new(1);
        let (a, b) = (sim.clock(), sim.clock());
        a.advance(Duration::from_secs(1));
        assert_eq!(b.now(), a.now());
    }

    #[test]
    fn streams_are_named_and_repeatable() {
        use hyalos_types::Entropy;
        let sim = Sim::new(7);
        assert_eq!(
            sim.stream("forge").next_u64(),
            sim.stream("forge").next_u64()
        );
        assert_ne!(
            sim.stream("forge").next_u64(),
            sim.stream("model").next_u64()
        );
    }
}
