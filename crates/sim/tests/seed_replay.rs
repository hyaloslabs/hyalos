//! The S1.5 property: a seed fixes the whole run (ADR-0013).
//!
//! The world here is a stand-in. Until the engine and its boundary interfaces exist (S1.1, S2.1), it
//! records, for each step, which boundary acted, when, and what it drew. The properties it proves are
//! the ones the real fakes will rely on.

use std::time::Duration;

use hyalos_sim::Sim;
use hyalos_types::{Clock, Entropy};

const BOUNDARIES: [&str; 3] = ["substrate", "forge", "model"];

type Trace = Vec<(u64, &'static str, u64)>;

/// Runs 300 steps. `extra_forge_draws` makes the forge stream draw more at every step, to show that
/// one boundary cannot disturb another.
fn run(seed: u64, extra_forge_draws: usize) -> Trace {
    let sim = Sim::new(seed);
    let clock = sim.clock();
    let mut order = sim.stream("order");
    let mut streams: Vec<_> = BOUNDARIES.iter().map(|b| sim.stream(b)).collect();
    let mut trace = Vec::new();
    for _ in 0..300 {
        let i = order.below(BOUNDARIES.len() as u64) as usize;
        clock.advance(Duration::from_millis(1 + order.below(50)));
        if BOUNDARIES[i] == "forge" {
            for _ in 0..extra_forge_draws {
                streams[i].next_u64();
            }
        }
        trace.push((
            clock.now().as_millis(),
            BOUNDARIES[i],
            streams[i].next_u64(),
        ));
    }
    trace
}

fn only(trace: &Trace, boundary: &str) -> Vec<u64> {
    trace
        .iter()
        .filter(|(_, b, _)| *b == boundary)
        .map(|(_, _, v)| *v)
        .collect()
}

#[test]
fn the_same_seed_gives_the_same_trace() {
    assert_eq!(run(42, 0), run(42, 0));
}

#[test]
fn many_seeds_each_replay_exactly() {
    for seed in 0..200 {
        assert_eq!(run(seed, 0), run(seed, 0), "seed {seed} did not replay");
    }
}

#[test]
fn different_seeds_give_different_traces() {
    assert_ne!(run(1, 0), run(2, 0));
}

#[test]
fn extra_draws_on_one_boundary_do_not_change_the_others() {
    let (plain, noisy) = (run(5, 0), run(5, 3));
    assert_ne!(only(&plain, "forge"), only(&noisy, "forge"));
    assert_eq!(only(&plain, "model"), only(&noisy, "model"));
    assert_eq!(only(&plain, "substrate"), only(&noisy, "substrate"));
}
