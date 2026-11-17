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

use std::path::PathBuf;

use hyalos_engine::{Engine, State};
use hyalos_plugin::sigstore::MAX_BUNDLE;
use hyalos_plugin::skill::digest_of;
use hyalos_plugin::snapshot::{BUNDLE, FILE, MAX_SNAPSHOT, Snapshot, SnapshotError};
use hyalos_types::{Clock, IndexLocation};

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
    let file = trust_store(env)?.join(format!("{}.json", last.digest));
    let bytes = std::fs::read(file).map_err(|_| unreadable())?;
    if digest_of(&bytes) != last.digest {
        return Err(unreadable());
    }
    Snapshot::kept(&bytes, last.signed_at).map_err(|_| unreadable())
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
    let file = match plugin_source::fetch(location, FILE, MAX_SNAPSHOT) {
        Ok(file) => file,
        // A directory without one, or a server that has none: nothing to import.
        Err(_) if !exists(location) => return Err(NotImported::Absent),
        Err(e) => return Err(NotImported::Failed(e)),
    };
    let bundle = plugin_source::fetch(location, BUNDLE, MAX_BUNDLE)
        .map_err(|_| NotImported::Refused(SnapshotError::Unsigned))?;
    let next = current
        .successor(&file, &bundle)
        .map_err(NotImported::Refused)?;
    let store = trust_store(env).map_err(NotImported::Failed)?;
    paths::make_private_dir(&store).map_err(NotImported::Failed)?;
    let kept = store.join(format!("{}.json", next.digest()));
    // Written beside it and renamed, so a half-written file is never found under its digest.
    let partial = store.join(format!(".partial-{}-{}", next.digest(), std::process::id()));
    std::fs::write(&partial, &file)
        .and_then(|()| std::fs::rename(&partial, &kept))
        .map_err(|e| NotImported::Failed(format!("cannot keep the snapshot: {e}")))?;
    let head = engine
        .import_snapshot(SystemClock.now(), next.digest(), next.signed_at())
        .map_err(|e| NotImported::Failed(e.to_string()))?;
    Ok((next, head.seq))
}

/// Whether `location` has a snapshot file at all, as far as can be told without fetching it: for a
/// directory, whether the file is there; a URL is assumed to have one, so a failed fetch is reported.
fn exists(location: &IndexLocation) -> bool {
    !location.is_dir() || std::path::Path::new(location.as_str()).join(FILE).exists()
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
