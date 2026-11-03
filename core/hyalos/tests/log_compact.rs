//! `hyalos log compact`, `prune` and `verify --archive` against the real binary (S1.7, ADR-0026).

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hyalos_engine::Engine;
use hyalos_log::LogKey;
use hyalos_types::{Digest, Event, Origin, Timestamp};

type Res<T = ()> = Result<T, Box<dyn Error>>;

const SEED: [u8; 32] = [9; 32];

struct Machine {
    root: PathBuf,
}

impl Machine {
    fn data(&self) -> PathBuf {
        self.root.join("data")
    }
    fn config(&self) -> PathBuf {
        self.root.join("config")
    }
    fn db(&self) -> PathBuf {
        self.data().join("events.db")
    }
    fn archives(&self) -> Res<Vec<String>> {
        let mut names: Vec<String> = match std::fs::read_dir(self.data().join("archive")) {
            Ok(dir) => dir
                .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect::<Result<_, _>>()?,
            Err(_) => Vec::new(),
        };
        names.sort();
        Ok(names)
    }
    fn run(&self, args: &[&str]) -> Res<Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_hyalos"))
            .args(args)
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("HOME")
            .env("HYALOS_DATA_DIR", self.data())
            .env("HYALOS_CONFIG_DIR", self.config())
            .output()?)
    }
    /// Adds `n` tasks to the log through the engine.
    fn add_tasks(&self, from: u64, n: u64) -> Res {
        let mut engine = Engine::open(&self.db(), LogKey::from_seed(SEED))?;
        for i in from..from + n {
            engine.submit(
                Timestamp::from_millis(1_000 + i),
                Origin::Engine,
                Event::TaskCreated {
                    request: Digest::from_bytes([i as u8; 32]),
                },
            )?;
        }
        Ok(())
    }
}

/// A machine with the engine's key in its config directory and a log of `n` tasks.
fn machine(name: &str, n: u64) -> Res<Machine> {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-compact-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    let m = Machine { root };
    std::fs::create_dir_all(m.data())?;
    std::fs::create_dir_all(m.config())?;
    let key = LogKey::from_seed(SEED);
    let key_file = m.config().join("engine.key");
    let seed_hex: String = SEED.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&key_file, format!("hyalos-engine-key-v1:{seed_hex}\n"))?;
    std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600))?;
    let public = m.config().join("engine.pub");
    std::fs::write(&public, format!("{}\n", key.public().to_hex()))?;
    std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o444))?;
    m.add_tasks(1, n)?;
    Ok(m)
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn ok(o: &Output) -> Res<String> {
    if o.status.success() {
        Ok(out(o))
    } else {
        Err(format!("failed: {} {}", out(o), err(o)).into())
    }
}

fn path_str(p: &Path) -> Res<&str> {
    p.to_str().ok_or_else(|| "path is not utf-8".into())
}

#[test]
fn compact_archives_the_events_and_verify_passes_across_the_snapshot() -> Res {
    let m = machine("happy", 5)?;
    let before = ok(&m.run(&["log", "verify"])?)?;
    assert!(before.contains("ok: 5 events, head 5:"), "{before}");
    let head = before
        .split("head ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .ok_or("no head")?
        .to_string();

    let result = m.run(&["log", "compact"])?;
    let text = ok(&result)?;
    assert!(text.contains("compacted: 5 events moved to"), "{text}");
    assert!(text.contains(&format!("snapshot at {head}")), "{text}");
    assert_eq!(m.archives()?, vec!["events-0000000001-0000000005.db"]);
    let archive = m.data().join("archive/events-0000000001-0000000005.db");
    assert_eq!(
        std::fs::metadata(&archive)?.permissions().mode() & 0o777,
        0o400
    );
    assert_eq!(
        std::fs::metadata(m.data().join("archive"))?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    // The same head, the same hash: only the way to get there changed.
    let after = ok(&m.run(&["log", "verify", "--expect-head", &head])?)?;
    assert!(
        after.contains(&format!(
            "ok: 0 events after the snapshot at {head}, head {head}"
        )),
        "{after}"
    );
    assert!(after.contains("rest on the engine's signature"), "{after}");

    // With the archive, the whole history is checked from the first event.
    let full = ok(&m.run(&["log", "verify", "--archive", path_str(&archive)?])?)?;
    assert!(
        full.contains("history from event 1 verified, through 1 archive(s)"),
        "{full}"
    );

    // The engine starts from the snapshot and carries on.
    m.add_tasks(6, 2)?;
    let later = ok(&m.run(&["log", "verify", "--archive", path_str(&archive)?])?)?;
    assert!(later.contains("2 events after the snapshot at"), "{later}");
    assert!(later.contains("history from event 1 verified"), "{later}");
    Ok(())
}

#[test]
fn a_second_compaction_archives_only_what_is_new_and_both_are_checked_together() -> Res {
    let m = machine("second", 3)?;
    ok(&m.run(&["log", "compact"])?)?;
    m.add_tasks(4, 4)?;
    ok(&m.run(&["log", "compact"])?)?;
    assert_eq!(
        m.archives()?,
        vec![
            "events-0000000001-0000000003.db",
            "events-0000000004-0000000007.db"
        ]
    );
    let a1 = m.data().join("archive/events-0000000001-0000000003.db");
    let a2 = m.data().join("archive/events-0000000004-0000000007.db");
    let (p1, p2) = (path_str(&a1)?, path_str(&a2)?);

    let both = ok(&m.run(&["log", "verify", "--archive", p1, "--archive", p2])?)?;
    assert!(
        both.contains("history from event 1 verified, through 2 archive(s)"),
        "{both}"
    );
    let one = ok(&m.run(&["log", "verify", "--archive", p2])?)?;
    assert!(one.contains("do not cover all of them"), "{one}");
    Ok(())
}

#[test]
fn there_is_nothing_to_compact_in_an_empty_log_or_twice_in_a_row() -> Res {
    let m = machine("nothing", 0)?;
    let first = m.run(&["log", "compact"])?;
    assert_eq!(first.status.code(), Some(1));
    assert!(
        err(&first).contains("no events since the last snapshot"),
        "{}",
        err(&first)
    );

    m.add_tasks(1, 2)?;
    ok(&m.run(&["log", "compact"])?)?;
    let again = m.run(&["log", "compact"])?;
    assert_eq!(again.status.code(), Some(1));
    assert!(
        err(&again).contains("no events since the last snapshot"),
        "{}",
        err(&again)
    );
    assert_eq!(m.archives()?.len(), 1, "no second archive");
    Ok(())
}

#[test]
fn compact_needs_the_engines_own_key_and_changes_nothing_without_it() -> Res {
    let m = machine("key", 4)?;
    let before = out(&m.run(&["log", "verify"])?);

    // Another machine's key: the log refuses it.
    let wrong = m.config().join("wrong.key");
    let seed_hex: String = [3u8; 32].iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&wrong, format!("hyalos-engine-key-v1:{seed_hex}\n"))?;
    std::fs::set_permissions(&wrong, std::fs::Permissions::from_mode(0o600))?;
    let result = m.run(&["log", "compact", "--key-file", path_str(&wrong)?])?;
    assert_eq!(result.status.code(), Some(1));
    assert!(
        err(&result).contains("different signing key"),
        "{}",
        err(&result)
    );

    // A key others can read is refused before anything is opened.
    let open = m.config().join("open.key");
    std::fs::write(&open, format!("hyalos-engine-key-v1:{}\n", "09".repeat(32)))?;
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644))?;
    let result = m.run(&["log", "compact", "--key-file", path_str(&open)?])?;
    assert_eq!(result.status.code(), Some(1));

    // No key at all.
    std::fs::remove_file(m.config().join("engine.key"))?;
    let result = m.run(&["log", "compact"])?;
    assert_eq!(result.status.code(), Some(1));

    assert_eq!(
        out(&m.run(&["log", "verify"])?),
        before,
        "the log is as it was"
    );
    assert!(m.archives()?.is_empty());
    Ok(())
}

#[test]
fn an_existing_archive_is_never_overwritten() -> Res {
    let m = machine("exists", 3)?;
    std::fs::create_dir_all(m.data().join("archive"))?;
    let taken = m.data().join("archive/events-0000000001-0000000003.db");
    std::fs::write(&taken, b"precious")?;
    let result = m.run(&["log", "compact"])?;
    assert_eq!(result.status.code(), Some(1));
    assert!(err(&result).contains("already exists"), "{}", err(&result));
    assert_eq!(std::fs::read(&taken)?, b"precious");
    assert!(out(&m.run(&["log", "verify"])?).contains("ok: 3 events, head 3:"));
    Ok(())
}

#[test]
fn prune_removes_one_archive_only_when_asked_and_only_if_it_belongs_to_the_log() -> Res {
    let m = machine("prune", 3)?;
    ok(&m.run(&["log", "compact"])?)?;
    m.add_tasks(4, 2)?;
    ok(&m.run(&["log", "compact"])?)?;
    let a1 = m.data().join("archive/events-0000000001-0000000003.db");
    let a2 = m.data().join("archive/events-0000000004-0000000005.db");
    let (p1, p2) = (path_str(&a1)?, path_str(&a2)?);

    // Without --yes it is a usage error and nothing is touched.
    let result = m.run(&["log", "prune", p1])?;
    assert_eq!(result.status.code(), Some(2));
    assert!(err(&result).contains("--yes"), "{}", err(&result));
    assert!(a1.exists());

    // The log is not an archive of itself.
    let result = m.run(&["log", "prune", path_str(&m.db())?, "--yes"])?;
    assert_eq!(result.status.code(), Some(1));
    assert!(m.db().exists());

    // A real archive goes, and the rest of the history still verifies as far as it can.
    let result = m.run(&["log", "prune", p1, "--yes"])?;
    let text = ok(&result)?;
    assert!(text.contains("(3 events, ending at 3:"), "{text}");
    assert!(text.contains("recorded in the log at event 6"), "{text}");
    assert!(!a1.exists());
    assert!(a2.exists(), "only the one named");
    let after = ok(&m.run(&["log", "verify", "--archive", p2])?)?;
    assert!(after.contains("do not cover all of them"), "{after}");

    // And an archive that was altered is refused, not deleted.
    let rotten = m.data().join("archive/rotten.db");
    std::fs::copy(&a2, &rotten)?;
    std::fs::set_permissions(&rotten, std::fs::Permissions::from_mode(0o600))?;
    rusqlite::Connection::open(&rotten)?.execute_batch(
        "DROP TRIGGER events_no_update; UPDATE events SET payload = x'7b7d' WHERE seq = 4;",
    )?;
    let result = m.run(&["log", "prune", path_str(&rotten)?, "--yes"])?;
    assert_eq!(result.status.code(), Some(1));
    assert!(rotten.exists());
    Ok(())
}

#[test]
fn a_prune_is_recorded_in_the_log_and_verify_lists_it_even_after_the_next_compaction() -> Res {
    let m = machine("recorded", 3)?;
    ok(&m.run(&["log", "compact"])?)?;
    let a1 = m.data().join("archive/events-0000000001-0000000003.db");

    let before = ok(&m.run(&["log", "verify"])?)?;
    assert!(!before.contains("pruned:"), "{before}");

    ok(&m.run(&["log", "prune", path_str(&a1)?, "--yes"])?)?;
    let after = ok(&m.run(&["log", "verify"])?)?;
    assert!(
        after.contains("pruned: the archive ending at event 3 (")
            && after.contains("recorded at event 4"),
        "{after}"
    );

    // Compacting moves that record into the snapshot, and into the new archive. It is still listed.
    ok(&m.run(&["log", "compact"])?)?;
    let later = ok(&m.run(&["log", "verify"])?)?;
    assert!(
        later.contains("pruned: the archive ending at event 3 (")
            && later.contains("0 events after the snapshot"),
        "{later}"
    );
    // The record is the engine's, signed with the rest: the log still verifies with the public key.
    let a2 = m.data().join("archive/events-0000000004-0000000004.db");
    let both = ok(&m.run(&["log", "verify", "--archive", path_str(&a2)?])?)?;
    assert!(
        both.contains("pruned: the archive ending at event 3 ("),
        "{both}"
    );
    Ok(())
}

#[test]
fn a_crash_between_the_record_and_the_delete_leaves_a_file_that_can_be_pruned_again() -> Res {
    let m = machine("crash", 3)?;
    let done = ok(&m.run(&["log", "compact"])?)?;
    let a1 = m.data().join("archive/events-0000000001-0000000003.db");
    // The engine records the prune and then "crashes": the file is still there.
    let head = {
        let mut engine = Engine::open(&m.db(), LogKey::from_seed(SEED))?;
        let snapshot = engine.snapshot_head()?.ok_or("no snapshot")?;
        engine.record_prune(Timestamp::from_millis(9_000), snapshot)?;
        snapshot
    };
    assert!(a1.exists(), "{done}");

    let text = ok(&m.run(&["log", "prune", path_str(&a1)?, "--yes"])?)?;
    assert!(text.contains("already recorded in the log"), "{text}");
    assert!(!a1.exists());
    // Recorded once, not twice.
    let engine = Engine::open(&m.db(), LogKey::from_seed(SEED))?;
    assert_eq!(engine.state().pruned().len(), 1);
    assert_eq!(engine.state().pruned()[0].last_seq, head.seq);
    Ok(())
}

#[test]
fn nothing_is_deleted_when_the_prune_cannot_be_recorded() -> Res {
    let m = machine("unrecorded", 3)?;
    ok(&m.run(&["log", "compact"])?)?;
    let a1 = m.data().join("archive/events-0000000001-0000000003.db");

    // Another key cannot sign into this log, so nothing may be removed.
    let wrong = m.root.join("other.key");
    let seed_hex: String = [7u8; 32].iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&wrong, format!("hyalos-engine-key-v1:{seed_hex}\n"))?;
    std::fs::set_permissions(&wrong, std::fs::Permissions::from_mode(0o600))?;
    let result = m.run(&[
        "log",
        "prune",
        path_str(&a1)?,
        "--yes",
        "--key-file",
        path_str(&wrong)?,
    ])?;
    assert_eq!(result.status.code(), Some(1), "{}", err(&result));
    assert!(a1.exists());

    // Without the engine's key at all, the same.
    std::fs::remove_file(m.config().join("engine.key"))?;
    let result = m.run(&["log", "prune", path_str(&a1)?, "--yes"])?;
    assert_eq!(result.status.code(), Some(1));
    assert!(a1.exists());
    Ok(())
}

#[test]
fn a_foreign_archive_is_refused_by_verify_and_prune() -> Res {
    let m = machine("foreign", 3)?;
    ok(&m.run(&["log", "compact"])?)?;
    let other = machine("foreign-other", 4)?;
    ok(&other.run(&["log", "compact"])?)?;
    let theirs = other.data().join("archive/events-0000000001-0000000004.db");
    let theirs = path_str(&theirs)?;

    let verify = m.run(&["log", "verify", "--archive", theirs])?;
    assert_eq!(verify.status.code(), Some(1));
    assert!(
        err(&verify).contains("not part of this log's history"),
        "{}",
        err(&verify)
    );
    let prune = m.run(&["log", "prune", theirs, "--yes"])?;
    assert_eq!(prune.status.code(), Some(1));
    assert!(
        err(&prune).contains("no snapshot of this log stands"),
        "{}",
        err(&prune)
    );
    assert!(Path::new(theirs).exists());
    assert!(
        !ok(&m.run(&["log", "verify"])?)?.contains("pruned:"),
        "nothing was recorded"
    );
    Ok(())
}

#[test]
fn backup_and_restore_carry_a_compacted_log() -> Res {
    let m = machine("backup", 4)?;
    ok(&m.run(&["log", "compact"])?)?;
    m.add_tasks(5, 1)?;
    let backup = m.root.join("backup.db");
    let b = path_str(&backup)?;
    ok(&m.run(&["log", "backup", b])?)?;

    let fresh = Machine {
        root: m.root.join("fresh"),
    };
    std::fs::create_dir_all(fresh.config())?;
    let public = LogKey::from_seed(SEED).public().to_hex();
    let restored = fresh.run(&["log", "restore", b, "--public-key", &public])?;
    let text = ok(&restored)?;
    assert!(text.contains("1 events after the snapshot at 4:"), "{text}");
    Ok(())
}
