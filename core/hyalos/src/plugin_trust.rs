//! The trusted-root snapshot in force, and importing a newer one (S7.11, ADR-0007, ADR-0097).
//!
//! The snapshot in force is the newest of the one this release embeds and the one the log says was
//! imported last, by issue time. An imported snapshot is kept in the data directory's `trust/` under
//! its digest and read back only if its bytes still have that digest. If it cannot be read, nothing
//! from an index is checked: the command refuses rather than fall back to the older embedded one.
//!
//! A newer snapshot is imported from an index location only if the snapshot in force vouches for it
//! ([`Snapshot::successor`]). The import is recorded as `plugin.snapshot_imported`, by the engine: it
//! is not a decision.

use std::io::Write;
use std::path::{Path, PathBuf};

use hyalos_engine::{Engine, State};
use hyalos_plugin::sigstore::MAX_BUNDLE;
use hyalos_plugin::skill::digest_of;
use hyalos_plugin::snapshot::{BUNDLE, FILE, MAX_SNAPSHOT, Snapshot, SnapshotError};
use hyalos_types::{Clock, Digest, IndexLocation};

use crate::clock::SystemClock;
use crate::paths::{self, Env};
use crate::plugin_source;

/// Where imported snapshots are kept, beside the plugin store.
pub const TRUST_DIR: &str = "trust";

fn trust_store(env: Env<'_>) -> Result<PathBuf, String> {
    Ok(paths::data_dir(env)?.join(TRUST_DIR))
}

/// The snapshot in force: `base`, the one this release embeds, unless the log records a later one
/// imported, which is then read back from the trust store and must still have its digest.
pub fn in_force(env: Env<'_>, state: Option<&State>, base: &Snapshot) -> Result<Snapshot, String> {
    let Some(last) = state.and_then(State::trusted_root) else {
        return Ok(base.clone());
    };
    if last.signed_at <= base.signed_at() {
        return Ok(base.clone());
    }
    let unreadable = || {
        format!(
            "the trusted-root snapshot imported at event {} cannot be read back with the digest the log recorded, so nothing from an index can be checked (snapshot.unreadable)",
            last.seq
        )
    };
    let bytes = std::fs::read(kept_file(env, last.digest)?).map_err(|_| unreadable())?;
    if digest_of(&bytes) != last.digest {
        return Err(unreadable());
    }
    Snapshot::kept(&bytes, last.signed_at).map_err(|_| unreadable())
}

fn kept_file(env: Env<'_>, digest: Digest) -> Result<PathBuf, String> {
    Ok(trust_store(env)?.join(format!("{digest}.json")))
}

/// Keeps `bytes` under `digest` in the trust store, durably: written beside it, flushed, renamed and
/// the directory flushed, so the event that names it is never written before it would survive a
/// crash, and a half-written file is never found under its digest.
fn keep(env: Env<'_>, digest: Digest, bytes: &[u8]) -> Result<(), String> {
    let store = trust_store(env)?;
    paths::make_private_dir(&store)?;
    let partial = store.join(format!(".partial-{digest}-{}", std::process::id()));
    let failed = |e: std::io::Error| format!("cannot keep the snapshot: {e}");
    let mut f = std::fs::File::create(&partial).map_err(failed)?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(failed)?;
    std::fs::rename(&partial, kept_file(env, digest)?).map_err(failed)?;
    sync_dir(&store).map_err(failed)
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Puts back the snapshot the log says was imported last, if its kept file cannot be read and
/// `location` holds a file with the digest the log recorded. Returns whether it did.
pub fn restore(env: Env<'_>, state: &State, location: &IndexLocation) -> Result<bool, String> {
    let Some(last) = state.trusted_root() else {
        return Ok(false);
    };
    let Some(bytes) = plugin_source::fetch_optional(location, FILE, MAX_SNAPSHOT)? else {
        return Ok(false);
    };
    if digest_of(&bytes) != last.digest {
        return Ok(false);
    }
    keep(env, last.digest, &bytes)?;
    Ok(true)
}

/// Why nothing was imported from a location.
pub enum NotImported {
    /// The location carries no snapshot.
    Absent,
    /// It carries one that was refused.
    Refused(SnapshotError),
    /// It could not be fetched or kept, or the import not recorded.
    Failed(String),
}

impl NotImported {
    /// The line an install prints about it, or nothing when there is nothing worth saying: no
    /// snapshot, or one no newer than the one in force.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Absent | Self::Refused(SnapshotError::NotNewer) => None,
            Self::Refused(e) => Some(format!(
                "the trusted-root snapshot there was not imported: {e} ({}); checking against the one in force\n",
                e.code()
            )),
            Self::Failed(e) => Some(format!(
                "the trusted-root snapshot there was not imported: {e}; checking against the one in force\n"
            )),
        }
    }
}

/// Imports the snapshot at `location` if `current` vouches for it and it is later, and returns it
/// with the event that recorded it.
pub fn import(
    env: Env<'_>,
    engine: &mut Engine,
    location: &IndexLocation,
    current: &Snapshot,
) -> Result<(Snapshot, u64), NotImported> {
    // A directory without one, or a server that answers 404: nothing to import.
    let file = plugin_source::fetch_optional(location, FILE, MAX_SNAPSHOT)
        .map_err(NotImported::Failed)?
        .ok_or(NotImported::Absent)?;
    let bundle = plugin_source::fetch(location, BUNDLE, MAX_BUNDLE)
        .map_err(|_| NotImported::Refused(SnapshotError::Unsigned))?;
    let next = current
        .successor(&file, &bundle)
        .map_err(NotImported::Refused)?;
    keep(env, next.digest(), &file).map_err(NotImported::Failed)?;
    let head = engine
        .import_snapshot(SystemClock.now(), next.digest(), next.signed_at())
        .map_err(|e| NotImported::Failed(e.to_string()))?;
    Ok((next, head.seq))
}

/// A snapshot as the operator reads it.
pub fn describe(snapshot: &Snapshot, imported_at: Option<u64>) -> String {
    let from = match imported_at {
        Some(seq) => format!("imported at event {seq}"),
        None => "embedded in this release".into(),
    };
    format!(
        "trusted-root snapshot {from}\ndigest {}\nissued {}, by the earliest signed time its Sigstore bundle proves\n",
        snapshot.digest(),
        crate::mcp::rfc3339(snapshot.signed_at().as_millis())
    )
}
