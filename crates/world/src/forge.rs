//! A fake forge: issues to read and pull requests to open, from a seeded stream (ADR-0013).

use std::time::Duration;

use hyalos_sim::FakeClock;
use hyalos_types::{Entropy, SeededRng};

use crate::Fault;
use crate::fixtures::Fixtures;
use crate::schedule::{Failing, Schedule};

/// What the broker asks of a forge. The real adapters will sit behind the same two calls; the token
/// is the broker's, never the sandbox's.
pub trait Forge {
    /// The raw text of an issue. It is attacker-controlled, so the caller treats it as tainted.
    fn read_issue(&mut self, token: &str, number: u64) -> Result<String, Fault>;
    /// Opens a pull request from `branch` and returns its head commit.
    fn open_pr(&mut self, token: &str, branch: &str) -> Result<PullRequest, Fault>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub head_sha: String,
}

/// Takes fake time and fails now and then, all decided by its own stream.
#[derive(Debug)]
pub struct FakeForge {
    rng: SeededRng,
    clock: FakeClock,
    token: String,
    content: String,
    faults: Failing,
    /// Requests served, faults included.
    pub requests: u32,
}

impl FakeForge {
    pub fn new(rng: SeededRng, clock: FakeClock, fixtures: &Fixtures) -> Self {
        Self {
            rng,
            clock,
            token: fixtures.forge_token.clone(),
            content: fixtures.forge_content.clone(),
            faults: Schedule::default().forge,
            requests: 0,
        }
    }

    /// Fails as the schedule says instead of at the usual rate.
    pub fn with_faults(mut self, faults: Failing) -> Self {
        self.faults = faults;
        self
    }

    fn begin(&mut self, token: &str) -> Result<(), Fault> {
        self.requests += 1;
        self.clock
            .advance(Duration::from_millis(5 + self.rng.below(60)));
        if token != self.token {
            return Err(Fault::Unauthorized);
        }
        // At the usual rate, about one request in ten fails.
        if self.faults.fails(self.requests, &mut self.rng) {
            return Err(Fault::Unavailable);
        }
        Ok(())
    }
}

impl Forge for FakeForge {
    fn read_issue(&mut self, token: &str, number: u64) -> Result<String, Fault> {
        self.begin(token)?;
        let words = 3 + self.rng.below(12);
        let filler: Vec<String> = (0..words)
            .map(|_| format!("w{:x}", self.rng.below(4096)))
            .collect();
        Ok(format!(
            "Issue #{number}: {}\n{}",
            filler.join(" "),
            self.content
        ))
    }

    fn open_pr(&mut self, token: &str, _branch: &str) -> Result<PullRequest, Fault> {
        self.begin(token)?;
        let (a, b) = (self.rng.next_u64(), self.rng.next_u64());
        Ok(PullRequest {
            number: 1 + self.rng.below(5000),
            head_sha: format!("{a:016x}{b:016x}"),
        })
    }
}
