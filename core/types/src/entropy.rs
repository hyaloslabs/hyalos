//! Randomness as an injected source, and a seeded generator for simulation (ADR-0013).
//!
//! The deterministic crates may not call a thread-local generator or the operating system. They are
//! handed an [`Entropy`]. The binary supplies one backed by the operating system; the simulator
//! supplies a [`SeededRng`], so a failing run replays from its seed.

/// A source of random 64-bit values.
pub trait Entropy {
    fn next_u64(&mut self) -> u64;
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// The SplitMix64 output function.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a, used only to turn a stream label into a number.
fn fnv1a(label: &str) -> u64 {
    label.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// A small, fast generator that is a pure function of its seed. It is not secure: never use it for
/// keys, nonces or anything an attacker must not guess.
#[derive(Debug, Clone)]
pub struct SeededRng {
    seed: u64,
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self { seed, state: seed }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// An independent stream named `label`. It depends only on this generator's seed and the label,
    /// not on how many numbers have been drawn or in which order streams were forked, so adding a
    /// draw to one boundary's stream never changes another's.
    pub fn fork(&self, label: &str) -> Self {
        Self::new(mix(self.seed ^ fnv1a(label)))
    }

    /// A value in `0..bound` without modulo bias. A `bound` of 0 is treated as 1 and returns 0.
    pub fn below(&mut self, bound: u64) -> u64 {
        let bound = bound.max(1);
        let limit = u64::MAX - (u64::MAX % bound);
        loop {
            let x = self.next_u64();
            if x < limit {
                return x % bound;
            }
        }
    }
}

impl Entropy for SeededRng {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN);
        mix(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(rng: &mut SeededRng, n: usize) -> Vec<u64> {
        (0..n).map(|_| rng.next_u64()).collect()
    }

    #[test]
    fn matches_the_published_splitmix64_values_for_seed_zero() {
        let mut rng = SeededRng::new(0);
        assert_eq!(
            draw(&mut rng, 3),
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F
            ]
        );
    }

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        assert_eq!(
            draw(&mut SeededRng::new(42), 100),
            draw(&mut SeededRng::new(42), 100)
        );
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        assert_ne!(
            draw(&mut SeededRng::new(1), 10),
            draw(&mut SeededRng::new(2), 10)
        );
    }

    #[test]
    fn a_fork_does_not_depend_on_how_much_the_parent_has_drawn() {
        let fresh = SeededRng::new(9);
        let mut used = SeededRng::new(9);
        draw(&mut used, 50);
        assert_eq!(
            draw(&mut fresh.fork("model"), 10),
            draw(&mut used.fork("model"), 10)
        );
    }

    #[test]
    fn forks_with_different_labels_differ() {
        let rng = SeededRng::new(9);
        assert_ne!(
            draw(&mut rng.fork("model"), 10),
            draw(&mut rng.fork("forge"), 10)
        );
    }

    #[test]
    fn below_stays_in_range_and_reaches_every_value() {
        let mut rng = SeededRng::new(3);
        let mut seen = [false; 6];
        for _ in 0..500 {
            let v = rng.below(6);
            assert!(v < 6);
            seen[v as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn a_zero_bound_returns_zero() {
        assert_eq!(SeededRng::new(1).below(0), 0);
    }
}
