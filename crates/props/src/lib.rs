//! Property-based checks on seeded streams (S8.3, ADR-0013, ADR-0109).
//!
//! A property is a function of a [`SeededRng`] that draws its own inputs and says what is wrong, if
//! anything. [`forall`] runs it on a range of seeds, one case per seed, so a failing case is named by
//! its seed and replays exactly with `HYALOS_PROP_SEED=<seed>`. Nothing here shrinks a failing
//! input: the seed is the reproduction, and the property's message says what it drew.
//!
//! The contract suites of the three boundaries use it: the substrate drivers
//! (`hyalos-substrate-contract`), the broker's enforcement (`hyalos-broker-contract`) and the plugin
//! host (`hyalos-plugin-contract`).

use std::ops::Range;

pub use hyalos_types::{Entropy, SeededRng};

/// The variable that replays one case: `HYALOS_PROP_SEED=<seed>`.
pub const SEED_VAR: &str = "HYALOS_PROP_SEED";
/// The variable that sets how many cases a property runs, overriding its default.
pub const CASES_VAR: &str = "HYALOS_PROP_CASES";

/// The seeds a property runs on: `HYALOS_PROP_SEED` alone when it is set, else `0..n`, where `n` is
/// `HYALOS_PROP_CASES` when set and `default` otherwise. A variable that does not parse is an error,
/// not a quiet return to the default.
pub fn seeds(default: u64) -> Result<Range<u64>, String> {
    let read = |name: &str| -> Result<Option<u64>, String> {
        match std::env::var(name) {
            Ok(text) => text
                .trim()
                .parse::<u64>()
                .map(Some)
                .map_err(|_| format!("{name}={text:?} is not a number")),
            Err(_) => Ok(None),
        }
    };
    if let Some(seed) = read(SEED_VAR)? {
        return Ok(seed..seed.saturating_add(1));
    }
    let n = read(CASES_VAR)?.unwrap_or(default);
    if n == 0 {
        return Err(format!("{CASES_VAR}=0 runs nothing"));
    }
    Ok(0..n)
}

/// Runs `property` once for each seed in `seeds` on a stream of its own, `label` forked from the
/// seed, and returns how many cases ran. The first failing case stops the run with its seed, the
/// property's message and the command that replays it.
pub fn forall(
    label: &str,
    seeds: Range<u64>,
    mut property: impl FnMut(&mut SeededRng) -> Result<(), String>,
) -> Result<u64, String> {
    let mut ran = 0;
    for seed in seeds {
        let mut rng = SeededRng::new(seed).fork(label);
        property(&mut rng).map_err(|why| {
            format!("{label}: case {seed} failed: {why}\n  replay it with {SEED_VAR}={seed}")
        })?;
        ran += 1;
    }
    Ok(ran)
}

/// One of `items`, chosen by one draw. `items` must not be empty.
pub fn pick<'a, T>(rng: &mut SeededRng, items: &'a [T]) -> &'a T {
    &items[rng.below(items.len() as u64) as usize]
}

/// Whether a one-in-`n` chance hit, by one draw.
pub fn one_in(rng: &mut SeededRng, n: u64) -> bool {
    rng.below(n) == 0
}

/// A number in `range`, by one draw. An empty range gives its start.
pub fn within(rng: &mut SeededRng, range: Range<u64>) -> u64 {
    range.start + rng.below(range.end.saturating_sub(range.start))
}

/// Up to `max` characters, each from `pool`, which must not be empty.
pub fn text(rng: &mut SeededRng, pool: &[char], max: u64) -> String {
    let len = rng.below(max + 1);
    (0..len).map(|_| *pick(rng, pool)).collect()
}

/// Each of `items` kept or not on a draw of its own, in order.
pub fn subset<T: Clone>(rng: &mut SeededRng, items: &[T]) -> Vec<T> {
    items.iter().filter(|_| one_in(rng, 2)).cloned().collect()
}

/// Letters and digits.
pub const ALPHANUMERIC: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L',
    'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '0', '1', '2', '3', '4',
    '5', '6', '7', '8', '9',
];

/// What a shell, a parser or a template would read as something else: quotes, expansions,
/// separators, escapes, whitespace and a few characters outside ASCII. No NUL.
pub const AWKWARD: &[char] = &[
    ' ', '\t', '\n', '\'', '"', '`', '$', '(', ')', '{', '}', ';', '&', '|', '<', '>', '*', '?',
    '[', ']', '\\', '=', '#', '~', '!', '%', '-', '/', ':', ',', '.', 'é', '✓', '\u{202e}', 'a',
    'Z', '0',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_fixes_every_draw() {
        let draw = |seed| {
            let mut rng = SeededRng::new(seed).fork("t");
            (
                text(&mut rng, AWKWARD, 20),
                within(&mut rng, 5..9),
                subset(&mut rng, &[1, 2, 3, 4]),
            )
        };
        assert_eq!(draw(3), draw(3));
        assert_ne!(draw(3), draw(4));
    }

    #[test]
    fn forall_names_the_failing_seed_and_how_to_replay_it() {
        let mut seen = Vec::new();
        let err = forall("p", 0..10, |rng| {
            seen.push(rng.seed());
            if seen.len() == 4 {
                Err("drew badly".into())
            } else {
                Ok(())
            }
        })
        .err();
        assert_eq!(seen.len(), 4);
        let err = err.unwrap_or_default();
        assert!(err.contains("case 3 failed: drew badly"), "{err}");
        assert!(err.contains("HYALOS_PROP_SEED=3"), "{err}");
        assert_eq!(forall("p", 0..10, |_| Ok(())), Ok(10));
    }

    #[test]
    fn generators_stay_in_their_bounds() {
        let mut rng = SeededRng::new(9);
        for _ in 0..1000 {
            assert!((5..9).contains(&within(&mut rng, 5..9)));
            assert!(text(&mut rng, ALPHANUMERIC, 6).chars().count() <= 6);
        }
        assert_eq!(within(&mut rng, 4..4), 4);
    }
}
