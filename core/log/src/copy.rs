//! Copying a log to a new file: backup and restore (ADR-0021).
//!
//! The copy is taken by SQLite itself (`VACUUM INTO`), which reads one consistent snapshot even while
//! the engine is appending, and which includes events still in the write-ahead log. A plain file copy
//! of a database in WAL mode can silently lose the newest events. The copy is written under a
//! temporary name, verified against the public key, and only then renamed into place, so a copy that
//! fails verification never exists under the name an operator would trust.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::key::PublicKey;
use crate::store::Head;
use crate::verify::{Verified, VerifyError, verify};

#[derive(Debug)]
pub enum CopyError {
    /// There is no log at the source.
    SourceMissing(PathBuf),
    /// The destination, or its temporary name, already exists. Nothing is overwritten.
    Exists(PathBuf),
    /// The copy was made but does not verify, so it was removed. The source is untouched.
    Verify(VerifyError),
    /// SQLite or the filesystem failed.
    Storage(String),
}

impl fmt::Display for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyError::SourceMissing(p) => write!(f, "there is no event log at {}", p.display()),
            CopyError::Exists(p) => write!(
                f,
                "{} already exists; a copy never overwrites, so move it aside or choose another name",
                p.display()
            ),
            CopyError::Verify(e) => write!(
                f,
                "the copy does not verify, so it was removed and the source was not touched: {e}"
            ),
            CopyError::Storage(e) => write!(f, "cannot copy the event log: {e}"),
        }
    }
}

impl std::error::Error for CopyError {}

impl From<rusqlite::Error> for CopyError {
    fn from(e: rusqlite::Error) -> Self {
        CopyError::Storage(e.to_string())
    }
}

impl From<std::io::Error> for CopyError {
    fn from(e: std::io::Error) -> Self {
        CopyError::Storage(e.to_string())
    }
}

fn partial_name(dst: &Path) -> PathBuf {
    let mut name: OsString = dst.file_name().map(OsString::from).unwrap_or_default();
    name.push(".partial");
    dst.with_file_name(name)
}

fn exists(path: &Path) -> bool {
    // Not `Path::exists`, which follows symlinks and says "no" for a dangling one.
    fs::symlink_metadata(path).is_ok()
}

/// Copies the log at `src` to a new file `dst`, then checks the copy against `key` (and `expect_head`,
/// if given) exactly as `hyalos log verify` would. `dst` is created owner-only, as a single file with
/// no write-ahead log beside it. It must not exist, and its directory must.
pub fn copy_verified(
    src: &Path,
    dst: &Path,
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<Verified, CopyError> {
    if !exists(src) {
        return Err(CopyError::SourceMissing(src.to_path_buf()));
    }
    let partial = partial_name(dst);
    for path in [dst, partial.as_path()] {
        if exists(path) {
            return Err(CopyError::Exists(path.to_path_buf()));
        }
    }

    // VACUUM INTO wants a file that does not exist or is empty; creating it ourselves, exclusively,
    // lets us set its mode before SQLite writes anything into it.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&partial)?;

    match write_and_verify(src, &partial, key, expect_head) {
        Ok(verified) => {
            let published = File::open(&partial)
                .and_then(|f| f.sync_all())
                .and_then(|()| fs::rename(&partial, dst))
                .and_then(|()| match dst.parent() {
                    Some(dir) if !dir.as_os_str().is_empty() => File::open(dir)?.sync_all(),
                    _ => Ok(()),
                });
            match published {
                Ok(()) => Ok(verified),
                Err(e) => {
                    let _ = fs::remove_file(&partial);
                    Err(e.into())
                }
            }
        }
        Err(e) => {
            let _ = fs::remove_file(&partial);
            Err(e)
        }
    }
}

fn write_and_verify(
    src: &Path,
    partial: &Path,
    key: &PublicKey,
    expect_head: Option<Head>,
) -> Result<Verified, CopyError> {
    let target = partial
        .to_str()
        .ok_or_else(|| CopyError::Storage("the destination path is not valid UTF-8".into()))?;
    {
        let source = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        source.busy_timeout(std::time::Duration::from_secs(5))?;
        source.execute("VACUUM INTO ?1", [target])?;
    }
    {
        // A single self-contained file: leave write-ahead mode so no -wal or -shm sits beside it.
        let copy = Connection::open(partial)?;
        copy.pragma_update(None, "journal_mode", "DELETE")?;
    }
    verify(partial, key, expect_head).map_err(CopyError::Verify)
}
