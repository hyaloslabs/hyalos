//! A fake model provider: replies and token counts from a seeded stream (ADR-0013).

use std::time::Duration;

use hyalos_sim::FakeClock;
use hyalos_types::SeededRng;

use crate::Fault;
use crate::fixtures::Fixtures;
use crate::schedule::{Failing, Schedule};

/// What the broker asks of a model provider. The key is the broker's, never the sandbox's.
pub trait Model {
    fn complete(&mut self, key: &str, prompt: &str) -> Result<Reply, Fault>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Raw model text. It never enters the log.
    pub text: String,
    pub tokens: u64,
}

#[derive(Debug)]
pub struct FakeModel {
    rng: SeededRng,
    clock: FakeClock,
    key: String,
    text: String,
    forge_content: String,
    faults: Failing,
    /// Calls served, faults included.
    pub calls: u32,
    /// Calls whose prompt carried the planted forge content, which shows tainted text did reach the
    /// model in the run.
    pub saw_forge_content: u32,
}

impl FakeModel {
    pub fn new(rng: SeededRng, clock: FakeClock, fixtures: &Fixtures) -> Self {
        Self {
            rng,
            clock,
            key: fixtures.provider_key.clone(),
            text: fixtures.model_text.clone(),
            forge_content: fixtures.forge_content.clone(),
            faults: Schedule::default().model,
            calls: 0,
            saw_forge_content: 0,
        }
    }

    /// Fails as the schedule says instead of at the usual rate.
    pub fn with_faults(mut self, faults: Failing) -> Self {
        self.faults = faults;
        self
    }
}

impl Model for FakeModel {
    fn complete(&mut self, key: &str, prompt: &str) -> Result<Reply, Fault> {
        self.calls += 1;
        self.clock
            .advance(Duration::from_millis(20 + self.rng.below(400)));
        if key != self.key {
            return Err(Fault::Unauthorized);
        }
        if prompt.contains(&self.forge_content) {
            self.saw_forge_content += 1;
        }
        // At the usual rate, about one call in eight fails.
        if self.faults.fails(self.calls, &mut self.rng) {
            return Err(Fault::Unavailable);
        }
        let tokens = 500 + self.rng.below(120_000) + prompt.len() as u64 / 4;
        Ok(Reply {
            text: format!("{} (#{:x})", self.text, self.rng.below(65_536)),
            tokens,
        })
    }
}
