//! The contract every substrate driver must pass (S2.1, ADR-0011, ADR-0032).
//!
//! `run` takes a driver and a [`Fixture`] and returns a [`Report`]. The contract has two halves:
//!
//! * **Behaviour**, which every driver must pass: exit codes, output, environment, deadlines,
//!   stopping, cleaning up, refusing a bad spec or a missing image.
//! * **Isolation**, which a driver that claims to isolate must pass: no network, no root, a read-only
//!   root filesystem, the broker's socket as the one way out, and an init as PID 1 that reaps orphans. A fake that runs programs on the host
//!   skips these and says so in the report: a skip is never a pass.
//!
//! Behaviour also has a generated half (S8.3, ADR-0109): properties checked on specs a seeded generator
//! draws, so every driver is held to the same properties on many specs, not only the ones written
//! out. A failing case names its seed and replays with `HYALOS_PROP_SEED=<seed>`.
//!
//! The programs it runs are `sh` and `perl` scripts, so the image needs both. They are short and
//! use no network.

mod checks;
mod fake;
mod properties;

pub use checks::run;
pub use fake::LocalProcessSubstrate;

use std::path::PathBuf;

/// What the contract needs to know about the driver's environment.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// An image the driver can start, with `sh` and `perl` in it.
    pub image: String,
    /// An image the driver must refuse to start.
    pub missing_image: String,
    /// An empty directory the contract may use for the broker socket.
    pub work_dir: PathBuf,
    /// Run the isolation half too. Set for any driver that claims to isolate.
    pub isolation: bool,
}

/// What a run of the contract found.
#[derive(Debug, Default)]
pub struct Report {
    pub passed: Vec<&'static str>,
    pub failed: Vec<(&'static str, String)>,
    pub skipped: Vec<(&'static str, &'static str)>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut out = format!(
            "{} passed, {} failed, {} skipped\n",
            self.passed.len(),
            self.failed.len(),
            self.skipped.len()
        );
        for (name, why) in &self.failed {
            out.push_str(&format!("  FAILED {name}: {why}\n"));
        }
        for (name, why) in &self.skipped {
            out.push_str(&format!("  skipped {name}: {why}\n"));
        }
        out
    }
}
