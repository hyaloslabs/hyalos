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
//!
//! The revocation list (S7.7, ADR-0098) travels the same way: kept in `trust/` under its digest,
//! imported from a location only if the snapshot in force verifies it as the pipeline's and it is
//! later than the one before (`plugin.revocations_imported`). Each installed release it revokes is
//! then recorded as `plugin.revoked`, and no job is given it.

use std::io::Write;
use std::path::{Path, PathBuf};

use hyalos_engine::{Engine, State};
use hyalos_plugin::revocation::{self, RevocationError, Revocations};
use hyalos_plugin::sigstore::MAX_BUNDLE;
use hyalos_plugin::skill::digest_of;
use hyalos_plugin::snapshot::{BUNDLE, FILE, MAX_SNAPSHOT, Snapshot, SnapshotError};
use hyalos_types::plugin::{DEFAULT_MAX_REVOCATION_AGE_DAYS, DEFAULT_MAX_SNAPSHOT_AGE_DAYS};
use hyalos_types::{Clock, Digest, IndexLocation, InstallPolicy, Timestamp};

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
    restore_kept(env, location, FILE, MAX_SNAPSHOT, last.digest)
}

/// Puts back the file `name` at `location` as the kept file for `digest`, if it has that digest.
fn restore_kept(
    env: Env<'_>,
    location: &IndexLocation,
    name: &str,
    limit: u64,
    digest: Digest,
) -> Result<bool, String> {
    let Some(bytes) = plugin_source::fetch_optional(location, name, limit)? else {
        return Ok(false);
    };
    if digest_of(&bytes) != digest {
        return Ok(false);
    }
    keep(env, digest, &bytes)?;
    Ok(true)
}

/// The revocation list in force: the one the log says was imported last, read back from the trust
/// store with the digest the log recorded, or `None` if none was imported. If it cannot be read,
/// nothing is installed (`revocations.unreadable`), since what it revokes cannot be told.
pub fn revocations_in_force(
    env: Env<'_>,
    state: Option<&State>,
) -> Result<Option<Revocations>, String> {
    revocations_kept_in(&trust_store(env)?, state)
}

/// As [`revocations_in_force`], with the trust store at `store`.
pub fn revocations_kept_in(
    store: &Path,
    state: Option<&State>,
) -> Result<Option<Revocations>, String> {
    let Some(last) = state.and_then(State::revocations) else {
        return Ok(None);
    };
    let unreadable = || {
        format!(
            "the revocation list imported at event {} cannot be read back with the digest the log recorded, so nothing can be installed; import it again with hyalos plugin snapshot import <location> (revocations.unreadable)",
            last.seq
        )
    };
    let bytes =
        std::fs::read(store.join(format!("{}.json", last.digest))).map_err(|_| unreadable())?;
    if digest_of(&bytes) != last.digest {
        return Err(unreadable());
    }
    Revocations::kept(&bytes, last.signed_at)
        .map(Some)
        .map_err(|_| unreadable())
}

/// Puts back the revocation list the log says was imported last from `location`, as [`restore`]
/// does the snapshot.
pub fn restore_revocations(
    env: Env<'_>,
    state: &State,
    location: &IndexLocation,
) -> Result<bool, String> {
    let Some(last) = state.revocations() else {
        return Ok(false);
    };
    restore_kept(
        env,
        location,
        revocation::FILE,
        revocation::MAX_LIST,
        last.digest,
    )
}

/// Imports the revocation list at `location` if `snapshot` verifies it as the pipeline's and it is
/// later than the one in force, then records each installed release it revokes. Returns the list,
/// the event that imported it and a line for each release disabled.
pub fn import_revocations(
    env: Env<'_>,
    engine: &mut Engine,
    location: &IndexLocation,
    snapshot: &Snapshot,
) -> Result<(Revocations, u64, String), NotImported> {
    let file = plugin_source::fetch_optional(location, revocation::FILE, revocation::MAX_LIST)
        .map_err(NotImported::Failed)?
        .ok_or(NotImported::Absent)?;
    let bundle = plugin_source::fetch(location, revocation::BUNDLE, MAX_BUNDLE)
        .map_err(|_| NotImported::RefusedList(RevocationError::Unsigned))?;
    let after = engine.state().revocations().map(|r| r.signed_at);
    let list =
        Revocations::verify(snapshot, &file, &bundle, after).map_err(NotImported::RefusedList)?;
    keep(env, list.digest(), &file).map_err(NotImported::Failed)?;
    let head = engine
        .import_revocations(SystemClock.now(), list.digest(), list.signed_at())
        .map_err(|e| NotImported::Failed(e.to_string()))?;
    let disabled = apply(engine, &list).map_err(NotImported::Failed)?;
    Ok((list, head.seq, disabled))
}

/// Records `plugin.revoked` for each installed release `list` revokes that is not recorded yet, and
/// returns a line for each.
pub fn apply(engine: &mut Engine, list: &Revocations) -> Result<String, String> {
    let hit: Vec<_> = engine
        .state()
        .plugins()
        .iter()
        .filter(|(name, _)| !engine.state().revoked_plugins().contains_key(*name))
        .filter_map(|(name, p)| Some((name.clone(), p.digest, list.reason(&p.digest)?.clone())))
        .collect();
    let mut out = String::new();
    for (name, digest, reason) in hit {
        let head = engine
            .revoke_plugin(SystemClock.now(), name.clone(), digest, reason.clone())
            .map_err(|e| e.to_string())?;
        out.push_str(&format!(
            "{name}: the installed release {digest} is revoked ({reason}), recorded at event {}: no job is given it until you install a release that is not revoked\n",
            head.seq
        ));
    }
    Ok(out)
}

/// [`apply`]s the list in force, if there is one: a release it revokes that an interrupted import
/// left unmarked is marked now.
pub fn apply_in_force(env: Env<'_>, engine: &mut Engine) -> Result<String, String> {
    match revocations_in_force(env, Some(engine.state()))? {
        Some(list) => apply(engine, &list),
        None => Ok(String::new()),
    }
}

const DAY_MS: u64 = 86_400_000;

/// Whether what a release is checked against is recent enough to install or update from (S7.6,
/// ADR-0007, ADR-0099): the snapshot in force issued at most `max_snapshot_age_days` before `now`,
/// and a revocation list in force issued at most `max_revocation_age_days` before it, by the install
/// policy or the defaults (90 and 30). Age is the host clock minus the signed time inside each. With
/// no list imported, what is revoked cannot be told, so nothing installs either. Installed plugins
/// are not touched: they keep running.
pub fn fresh(
    snapshot: &Snapshot,
    list: Option<&Revocations>,
    policy: Option<&InstallPolicy>,
    now: Timestamp,
) -> Result<(), String> {
    let (max_snapshot, max_list) = policy.map_or(
        (
            DEFAULT_MAX_SNAPSHOT_AGE_DAYS,
            DEFAULT_MAX_REVOCATION_AGE_DAYS,
        ),
        |p| (p.max_snapshot_age_days, p.max_revocation_age_days),
    );
    let old = |signed_at: Timestamp, days: u32| {
        now.as_millis().saturating_sub(signed_at.as_millis()) > u64::from(days) * DAY_MS
    };
    let refresh =
        "import a newer release with hyalos plugin snapshot import <location>, or install from one";
    if old(snapshot.signed_at(), max_snapshot) {
        return Err(format!(
            "the trusted-root snapshot in force was issued {}, more than {max_snapshot} days ago, so nothing new is installed or updated: {refresh} (snapshot.too_old)",
            crate::mcp::rfc3339(snapshot.signed_at().as_millis())
        ));
    }
    match list {
        None => Err(format!(
            "no revocation list has been imported, so what is revoked cannot be told and nothing new is installed or updated: {refresh} (revocations.absent)"
        )),
        Some(list) if old(list.signed_at(), max_list) => Err(format!(
            "the revocation list in force was issued {}, more than {max_list} days ago, so nothing new is installed or updated: {refresh} (revocations.too_old)",
            crate::mcp::rfc3339(list.signed_at().as_millis())
        )),
        Some(_) => Ok(()),
    }
}

/// A refusal of `digest` if the list in force revokes it (`plugin.revoked`).
pub fn refuse_revoked(list: Option<&Revocations>, digest: &Digest) -> Result<(), String> {
    match list.and_then(|l| l.reason(digest)) {
        Some(reason) => Err(format!(
            "release {digest} is revoked ({reason}) by the revocation list in force, so it was not installed (plugin.revoked)"
        )),
        None => Ok(()),
    }
}

/// A revocation list as the operator reads it.
pub fn describe_revocations(list: Option<&Revocations>, imported_at: Option<u64>) -> String {
    match (list, imported_at) {
        (Some(list), Some(seq)) => format!(
            "revocation list imported at event {seq}\ndigest {}\nissued {}\nrevokes {} release(s)\n",
            list.digest(),
            crate::mcp::rfc3339(list.signed_at().as_millis()),
            list.len()
        ),
        _ => "no revocation list has been imported\n".into(),
    }
}

/// Why nothing was imported from a location.
pub enum NotImported {
    /// The location carries no snapshot.
    Absent,
    /// It carries a snapshot that was refused.
    Refused(SnapshotError),
    /// It carries a revocation list that was refused.
    RefusedList(RevocationError),
    /// It could not be fetched or kept, or the import not recorded.
    Failed(String),
}

impl NotImported {
    /// Why, as one line.
    pub fn why(&self) -> String {
        match self {
            Self::Absent => "there is none there".into(),
            Self::Refused(e) => format!("{e} ({})", e.code()),
            Self::RefusedList(e) => format!("{e} ({})", e.code()),
            Self::Failed(e) => e.clone(),
        }
    }

    /// The line an install prints about it, or nothing when there is nothing worth saying: no
    /// snapshot, or one no newer than the one in force.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Absent
            | Self::Refused(SnapshotError::NotNewer)
            | Self::RefusedList(RevocationError::NotNewer) => None,
            Self::RefusedList(e) => Some(format!(
                "the revocation list there was not imported: {e} ({}); the one in force still applies\n",
                e.code()
            )),
            Self::Refused(e) => Some(format!(
                "the trusted-root snapshot there was not imported: {e} ({}); checking against the one in force\n",
                e.code()
            )),
            Self::Failed(e) => Some(format!(
                "a trust file there ({} or {}) was not imported: {e}; the ones in force still apply\n",
                FILE,
                revocation::FILE
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

#[cfg(test)]
mod tests {
    use super::*;

    const EMBEDDED_LIST: &[u8] = br#"{"kind":"hyalos.revocation_list","revoked":[],"version":1}"#;

    fn at_days(days: u64) -> Timestamp {
        Timestamp::from_millis(days * DAY_MS)
    }

    #[test]
    fn what_a_release_is_checked_against_has_a_maximum_age() {
        let snapshot = |days| {
            Snapshot::kept(
                include_bytes!("../../plugin/trust/trusted-root.json"),
                at_days(days),
            )
            .unwrap()
        };
        let list = |days| Revocations::kept(EMBEDDED_LIST, at_days(days)).unwrap();
        let now = at_days(1000);
        assert!(fresh(&snapshot(910), Some(&list(970)), None, now).is_ok());
        let code = |r: Result<(), String>| r.unwrap_err();
        assert!(
            code(fresh(&snapshot(909), Some(&list(970)), None, now)).contains("(snapshot.too_old)")
        );
        assert!(
            code(fresh(&snapshot(910), Some(&list(969)), None, now))
                .contains("(revocations.too_old)")
        );
        assert!(code(fresh(&snapshot(910), None, None, now)).contains("(revocations.absent)"));
        // The operator's policy sets the limits.
        let policy = InstallPolicy {
            ceiling: Default::default(),
            indexes: Default::default(),
            max_snapshot_age_days: 10,
            max_revocation_age_days: 365,
        };
        assert!(
            code(fresh(&snapshot(980), Some(&list(700)), Some(&policy), now))
                .contains("snapshot.too_old")
        );
        assert!(fresh(&snapshot(995), Some(&list(700)), Some(&policy), now).is_ok());
        // A signed time after the host clock is not old.
        assert!(fresh(&snapshot(2000), Some(&list(2000)), None, now).is_ok());
    }
}
