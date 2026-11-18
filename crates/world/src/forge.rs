//! A fake forge: issues to read and pull requests to open, from a seeded stream (ADR-0013).

use std::collections::BTreeMap;
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
    /// Pushes a commit to pull request `pr`'s branch and returns its new head.
    fn push(&mut self, token: &str, pr: u64) -> Result<String, Fault>;
    /// Merges pull request `pr` if its head is still `sha`, as GitHub does when it is given the
    /// commit the operator approved (ADR-0071).
    fn merge(&mut self, token: &str, pr: u64, sha: &str) -> Result<Merged, Fault>;
}

/// What a forge answered a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Merged at the commit asked for.
    At(String),
    /// Refused: the head is no longer the commit asked for (`merge.head_moved`).
    HeadMoved,
}

/// A merge the forge made: the commit it was asked to merge, and the pull request's head then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeMerge {
    pub pr: u64,
    pub sha: String,
    pub head: String,
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
    /// Each open pull request's head.
    pub heads: BTreeMap<u64, String>,
    /// Every merge, in order.
    pub merges: Vec<ForgeMerge>,
}

/// A 40-digit commit id made from `seed` alone, so naming a commit draws nothing from the stream.
fn commit_id(seed: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(seed);
    digest.iter().take(20).map(|b| format!("{b:02x}")).collect()
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
            heads: BTreeMap::new(),
            merges: Vec::new(),
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
        let pr = PullRequest {
            number: 1 + self.rng.below(5000),
            head_sha: commit_id(format!("{a:016x}{b:016x}").as_bytes()),
        };
        self.heads.insert(pr.number, pr.head_sha.clone());
        Ok(pr)
    }

    fn push(&mut self, token: &str, pr: u64) -> Result<String, Fault> {
        self.begin(token)?;
        let old = self.heads.get(&pr).ok_or(Fault::Unavailable)?;
        let new = commit_id(format!("{old}+{}", self.requests).as_bytes());
        self.heads.insert(pr, new.clone());
        Ok(new)
    }

    fn merge(&mut self, token: &str, pr: u64, sha: &str) -> Result<Merged, Fault> {
        self.begin(token)?;
        let head = self.heads.get(&pr).ok_or(Fault::Unavailable)?.clone();
        if head != sha {
            return Ok(Merged::HeadMoved);
        }
        self.heads.remove(&pr);
        self.merges.push(ForgeMerge {
            pr,
            sha: sha.to_string(),
            head,
        });
        Ok(Merged::At(sha.to_string()))
    }
}
