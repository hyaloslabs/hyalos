//! A simulated world for the engine (S1.5, S1.1, ADR-0013, ADR-0036).
//!
//! A fake forge, a fake model provider, a fake substrate and a seeded gate stand where the real ones
//! will stand, all drawing from the streams of one [`hyalos_sim::Sim`] and all taking time from its
//! clock. [`run_job`] drives one job through the real engine, the real policy, the real manifest
//! signing and the real broker admission, so a seed fixes the whole run and a failing run replays.
//!
//! Every run plants secrets and raw content ([`Fixtures`]) where the real ones would be, and [`scan`]
//! looks for them in the files the engine wrote.

mod fixtures;
mod forge;
mod gate;
mod model;
mod run;
mod scan;
mod substrate;

pub use fixtures::Fixtures;
pub use forge::{FakeForge, Forge, PullRequest};
pub use gate::SeededGate;
pub use model::{FakeModel, Model, Reply};
pub use run::{Counts, Run, TOKEN_BUDGET, run_job};
pub use scan::{Leak, scan};
pub use substrate::SeededSubstrate;

/// Why a boundary did not do what it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// It failed or could not be reached.
    Unavailable,
    /// It was given a token or key that is not the one it holds. A run that sees this has a bug.
    Unauthorized,
}
