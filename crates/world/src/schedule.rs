//! Fault schedules: how often, and from when, each boundary fails in one run (S8.1, ADR-0013,
//! ADR-0104).
//!
//! A [`Schedule`] is drawn from the seed's own `schedule` stream, so the seed alone fixes both the
//! schedule and the run. [`Schedule::default`] is the fixed rates of ADR-0036, which `run_job` keeps,
//! so a seed run without a schedule does exactly what it did before schedules existed.

use std::fmt;

use hyalos_sim::Sim;
use hyalos_types::SeededRng;

/// `hits` chances in `of`: one draw of `below(of)`, a hit when it is under `hits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Odds {
    pub hits: u64,
    pub of: u64,
}

impl Odds {
    pub const NEVER: Self = Self { hits: 0, of: 1 };
    pub const ALWAYS: Self = Self { hits: 1, of: 1 };

    pub const fn new(hits: u64, of: u64) -> Self {
        Self { hits, of }
    }

    /// Draws once and says whether it hit.
    pub fn hit(self, rng: &mut SeededRng) -> bool {
        rng.below(self.of) < self.hits
    }
}

impl fmt::Display for Odds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.hits, self.of)
    }
}

/// How a forge or a model provider fails: at random, and from some call on for good (an outage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failing {
    pub odds: Odds,
    /// Every call after this many fails, without a draw.
    pub down_after: Option<u32>,
}

impl Failing {
    pub const NEVER: Self = Self {
        odds: Odds::NEVER,
        down_after: None,
    };

    pub const fn at(hits: u64, of: u64) -> Self {
        Self {
            odds: Odds::new(hits, of),
            down_after: None,
        }
    }

    /// Whether call number `call` (counted from 1) fails.
    pub fn fails(self, call: u32, rng: &mut SeededRng) -> bool {
        if self.down_after.is_some_and(|after| call > after) {
            return true;
        }
        self.odds.hit(rng)
    }
}

impl fmt::Display for Failing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fails {}", self.odds)?;
        if let Some(after) = self.down_after {
            write!(f, ", down after {after} calls")?;
        }
        Ok(())
    }
}

/// What a gate answers, as slots out of `of`: a fault, a timeout, unknown, deny, and allow for the
/// rest. One draw picks the slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateMix {
    pub unavailable: u64,
    pub timed_out: u64,
    pub unknown: u64,
    pub deny: u64,
    pub of: u64,
    /// Answers slow enough to miss the caller's deadline.
    pub slow: Odds,
    /// Every question after this many is a fault, without a draw.
    pub down_after: Option<u32>,
}

impl fmt::Display for GateMix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unavailable {u}/{n}, timed out {t}/{n}, unknown {k}/{n}, deny {d}/{n}, slow {s}",
            u = self.unavailable,
            t = self.timed_out,
            k = self.unknown,
            d = self.deny,
            n = self.of,
            s = self.slow
        )?;
        if let Some(after) = self.down_after {
            write!(f, ", down after {after} questions")?;
        }
        Ok(())
    }
}

/// How a sandbox ends, as slots out of `of`, and how often the runtime refuses to start one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxMix {
    pub refused: Odds,
    pub deadline: u64,
    pub lost: u64,
    pub failed: u64,
    pub of: u64,
}

impl fmt::Display for SandboxMix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "refused {r}, deadline {d}/{n}, lost {l}/{n}, exit 1 {x}/{n}",
            r = self.refused,
            d = self.deadline,
            l = self.lost,
            x = self.failed,
            n = self.of
        )
    }
}

/// The boundary an outage takes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Boundary {
    Forge,
    Model,
    Gate,
    Substrate,
}

/// The kind of schedule a seed drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    /// Nothing fails, and every gate allows on time.
    Calm,
    /// The fixed rates of ADR-0036.
    Usual,
    /// The usual rates, and one boundary down for good after a few calls.
    Outage(Boundary),
    /// Every rate drawn, from none up to half the calls.
    Storm,
}

impl Profile {
    /// Every profile a sweep must reach.
    pub const ALL: [Self; 7] = [
        Self::Calm,
        Self::Usual,
        Self::Outage(Boundary::Forge),
        Self::Outage(Boundary::Model),
        Self::Outage(Boundary::Gate),
        Self::Outage(Boundary::Substrate),
        Self::Storm,
    ];
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Calm => f.write_str("calm"),
            Self::Usual => f.write_str("usual"),
            Self::Outage(b) => write!(f, "{} outage", format!("{b:?}").to_lowercase()),
            Self::Storm => f.write_str("storm"),
        }
    }
}

/// When and how each boundary fails in one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub profile: Profile,
    pub forge: Failing,
    pub model: Failing,
    pub gate: GateMix,
    pub sandbox: SandboxMix,
}

impl Default for Schedule {
    /// The fixed rates of ADR-0036.
    fn default() -> Self {
        Self {
            profile: Profile::Usual,
            forge: Failing::at(1, 10),
            model: Failing::at(1, 8),
            gate: GateMix {
                unavailable: 1,
                timed_out: 1,
                unknown: 2,
                deny: 4,
                of: 40,
                slow: Odds::new(1, 25),
                down_after: None,
            },
            sandbox: SandboxMix {
                refused: Odds::new(1, 40),
                deadline: 1,
                lost: 1,
                failed: 2,
                of: 20,
            },
        }
    }
}

impl Schedule {
    /// Nothing fails and every gate allows on time.
    pub fn calm() -> Self {
        Self {
            profile: Profile::Calm,
            forge: Failing::NEVER,
            model: Failing::NEVER,
            gate: GateMix {
                unavailable: 0,
                timed_out: 0,
                unknown: 0,
                deny: 0,
                of: 1,
                slow: Odds::NEVER,
                down_after: None,
            },
            sandbox: SandboxMix {
                refused: Odds::NEVER,
                deadline: 0,
                lost: 0,
                failed: 0,
                of: 1,
            },
        }
    }

    /// The schedule for a seed, drawn from its `schedule` stream, which no fake draws from.
    pub fn for_seed(seed: u64) -> Self {
        let mut rng = Sim::new(seed).stream("schedule");
        match rng.below(8) {
            0 => Self::calm(),
            1 | 2 => Self::default(),
            3 | 4 => {
                let mut s = Self::default();
                let after = Some(rng.below(4) as u32);
                let boundary = match rng.below(4) {
                    0 => Boundary::Forge,
                    1 => Boundary::Model,
                    2 => Boundary::Gate,
                    _ => Boundary::Substrate,
                };
                match boundary {
                    Boundary::Forge => s.forge.down_after = after,
                    Boundary::Model => s.model.down_after = after,
                    Boundary::Gate => s.gate.down_after = after,
                    // A sandbox is started once, so its outage is a runtime that refuses it.
                    Boundary::Substrate => s.sandbox.refused = Odds::ALWAYS,
                }
                s.profile = Profile::Outage(boundary);
                s
            }
            _ => Self {
                profile: Profile::Storm,
                forge: Failing::at(rng.below(6), 10),
                model: Failing::at(rng.below(6), 10),
                gate: GateMix {
                    unavailable: rng.below(4),
                    timed_out: rng.below(4),
                    unknown: rng.below(6),
                    deny: rng.below(8),
                    of: 40,
                    slow: Odds::new(rng.below(5), 25),
                    down_after: None,
                },
                sandbox: SandboxMix {
                    refused: Odds::new(rng.below(6), 40),
                    deadline: rng.below(3),
                    lost: rng.below(3),
                    failed: rng.below(5),
                    of: 20,
                },
            },
        }
    }
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "profile:   {}", self.profile)?;
        writeln!(f, "forge:     {}", self.forge)?;
        writeln!(f, "model:     {}", self.model)?;
        writeln!(f, "gate:      {}", self.gate)?;
        write!(f, "sandbox:   {}", self.sandbox)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odds_hit_as_often_as_they_say() {
        let mut rng = SeededRng::new(1);
        assert!((0..100).all(|_| !Odds::NEVER.hit(&mut rng)));
        assert!((0..100).all(|_| Odds::ALWAYS.hit(&mut rng)));
        let hits = (0..10_000)
            .filter(|_| Odds::new(1, 4).hit(&mut rng))
            .count();
        assert!((2_200..2_800).contains(&hits), "{hits}");
    }

    #[test]
    fn an_outage_fails_every_call_after_it_starts() {
        let mut rng = SeededRng::new(2);
        let down = Failing {
            odds: Odds::NEVER,
            down_after: Some(2),
        };
        let calls: Vec<bool> = (1..=5).map(|n| down.fails(n, &mut rng)).collect();
        assert_eq!(calls, [false, false, true, true, true]);
    }

    #[test]
    fn a_seed_fixes_its_schedule_and_every_profile_occurs() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..200 {
            assert_eq!(Schedule::for_seed(seed), Schedule::for_seed(seed));
            seen.insert(Schedule::for_seed(seed).profile);
        }
        assert_eq!(seen.len(), Profile::ALL.len(), "{seen:?}");
    }
}
