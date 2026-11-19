//! The broker confines itself once it has read what it needs at startup (ADR-0118).
//!
//! Before confining, the broker reads `provider.keys`, connects to the engine and makes the plugin
//! runtime. With Landlock it then gives up every filesystem right except these:
//! - reading the system's own files (`/etc`, `/usr`, `/lib*`, `/proc`, `/sys`, `/run`, `/dev`), for
//!   name resolution and the C library;
//! - reading the plugin store, where a deterministic step's plugin is loaded (ADR-0102);
//! - making and removing sockets in the runtime directory, where each job's socket is bound.
//!
//! The config directory, which holds `engine.key`, `github-app.pem` and `operator.key`, is never
//! granted, and the engine refuses to start a broker whose read grants would reach it.

use std::path::{Path, PathBuf};

/// The system's own directories the broker may read. Those that do not exist are skipped.
pub const SYSTEM_READS: &[&str] = &[
    "/etc", "/usr", "/lib", "/lib32", "/lib64", "/proc", "/sys", "/run", "/dev",
];

/// What the broker may still reach once confined, besides [`SYSTEM_READS`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    /// Directories a job's socket is bound in: sockets may be made and removed there, nothing read.
    pub sockets: Vec<PathBuf>,
    /// Directories that may be read.
    pub reads: Vec<PathBuf>,
}

impl Grants {
    /// Every directory the broker may read.
    pub fn all_reads(&self) -> Vec<PathBuf> {
        SYSTEM_READS
            .iter()
            .map(PathBuf::from)
            .chain(self.reads.iter().cloned())
            .collect()
    }

    /// Whether a read grant contains `dir` or is `dir`, so the broker could read what is in it. Both
    /// are compared as resolved, since Landlock follows a link in a rule's path.
    pub fn may_read(&self, dir: &Path) -> bool {
        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let dir = real(dir);
        self.all_reads()
            .iter()
            .any(|read| dir.starts_with(real(read)))
    }
}

/// How far the confinement holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confined {
    /// Every right this kernel can restrict is restricted.
    Fully,
    /// The kernel's Landlock is older than the rules asked for; file reads and writes are still
    /// restricted, since the first Landlock version restricts them.
    Partly,
    /// This kernel has no Landlock, or it is turned off.
    Not,
}

#[cfg(target_os = "linux")]
pub fn apply(grants: &Grants) -> Result<Confined, String> {
    use landlock::{
        ABI, Access, AccessFs, BitFlags, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus,
        path_beneath_rules,
    };
    let abi = ABI::V3;
    let read: BitFlags<AccessFs> = AccessFs::ReadFile | AccessFs::ReadDir;
    let socket: BitFlags<AccessFs> = AccessFs::MakeSock | AccessFs::RemoveFile;
    let existing =
        |dirs: Vec<PathBuf>| -> Vec<PathBuf> { dirs.into_iter().filter(|d| d.exists()).collect() };
    let failed = |e: landlock::RulesetError| format!("cannot confine the broker: {e}");
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .map_err(failed)?
        .create()
        .map_err(failed)?
        .add_rules(path_beneath_rules(existing(grants.all_reads()), read))
        .map_err(failed)?
        .add_rules(path_beneath_rules(existing(grants.sockets.clone()), socket))
        .map_err(failed)?
        .restrict_self()
        .map_err(failed)?;
    Ok(match status.ruleset {
        RulesetStatus::FullyEnforced => Confined::Fully,
        RulesetStatus::PartiallyEnforced => Confined::Partly,
        RulesetStatus::NotEnforced => Confined::Not,
    })
}

/// Whether this kernel has Landlock turned on, so a broker started here confines itself. Asking
/// restricts nothing.
#[cfg(target_os = "linux")]
pub fn available() -> bool {
    use landlock::{ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| r.create())
        .is_ok()
}

#[cfg(not(target_os = "linux"))]
pub fn available() -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
pub fn apply(_grants: &Grants) -> Result<Confined, String> {
    Ok(Confined::Not)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_grant_reaches_what_is_beneath_it_and_nothing_beside_it() {
        let grants = Grants {
            sockets: vec![PathBuf::from("/home/o/.local/share/hyalos/run")],
            reads: vec![PathBuf::from("/home/o/.local/share/hyalos/plugins")],
        };
        assert!(grants.may_read(Path::new("/home/o/.local/share/hyalos/plugins/ab")));
        assert!(grants.may_read(Path::new("/etc/hyalos")));
        // The config directory, and the runtime directory, which is for sockets only.
        assert!(!grants.may_read(Path::new("/home/o/.config/hyalos")));
        assert!(!grants.may_read(Path::new("/home/o/.local/share/hyalos/run")));
        assert!(!grants.may_read(Path::new("/home/o/.local/share/hyalos/plugins-old")));
    }
}
