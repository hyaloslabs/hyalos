//! Keeping the signing key out of crash dumps (ADR-0030).
//!
//! A crash dump holds the process's memory, and so possibly the engine's key. The process switches
//! dumps off for itself at start, through `rustix`'s safe wrappers, since the workspace forbids
//! `unsafe`:
//!
//! - the core file size limit goes to zero, soft and hard, so nothing in the process can raise it
//!   again. This is what `ulimit -c 0` does, and every Unix honours it for a plain file;
//! - on Linux the process is also marked not dumpable. The kernel then never dumps it, whatever
//!   `core_pattern` says, which matters because a pipe to a crash handler (systemd-coredump,
//!   apport) is where most desktop and server dumps go and a pipe is not stopped by the size limit
//!   alone. The mark also stops other processes of the same user reading its memory with `ptrace` or
//!   `/proc/<pid>/mem`.

use std::fmt;

use rustix::process::{Resource, Rlimit, setrlimit};

/// Why the process could not be made safe from dumps.
#[derive(Debug)]
pub struct Failed {
    what: &'static str,
    error: String,
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot {}: {}. Without it a crash could write the signing key to disk (ADR-0030)",
            self.what, self.error
        )
    }
}

impl std::error::Error for Failed {}

/// Turns core dumps off for this process. Call it first, before any key is read.
pub fn no_core_dumps() -> Result<(), Failed> {
    setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .map_err(|e| Failed {
        what: "set the core file size limit to 0",
        error: e.to_string(),
    })?;
    #[cfg(target_os = "linux")]
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|e| Failed {
            what: "mark the process not dumpable",
            error: e.to_string(),
        })?;
    Ok(())
}
