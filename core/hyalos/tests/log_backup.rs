//! `hyalos log backup` and `restore` against the real binary (S1.3, ADR-0021).

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hyalos_log::{Log, LogKey, Origin};
use hyalos_types::Timestamp;
use rusqlite::Connection;

type Res<T = ()> = Result<T, Box<dyn Error>>;

struct Machine {
    root: PathBuf,
    key_hex: String,
    head: String,
}

fn machine(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-backup-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A machine with a log of four events in its data directory.
fn old_machine(name: &str) -> Res<Machine> {
    let root = machine(name)?;
    std::fs::create_dir_all(root.join("data"))?;
    let key = LogKey::from_seed([9; 32]);
    let key_hex = key.public().to_hex();
    let mut log = Log::open(&root.join("data/events.db"), key)?;
    let mut head = String::new();
    for i in 1..=4u64 {
        let origin = if i % 2 == 0 {
            Origin::Broker
        } else {
            Origin::Engine
        };
        head = log
            .append(Timestamp::from_millis(i), origin, "test", 1, b"{}")?
            .to_string();
    }
    Ok(Machine {
        root,
        key_hex,
        head,
    })
}

fn hyalos(args: &[&str], data: &Path, config: &Path) -> Res<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_hyalos"))
        .args(args)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME")
        .env("HYALOS_DATA_DIR", data)
        .env("HYALOS_CONFIG_DIR", config)
        .output()?)
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn back_up_on_one_machine_and_restore_on_a_fresh_one() -> Res {
    let old = old_machine("roundtrip-old")?;
    let backup = old.root.join("backup.db");
    let b = backup.to_str().ok_or("path")?;
    let result = hyalos(
        &[
            "log",
            "backup",
            b,
            "--public-key",
            &old.key_hex,
            "--expect-head",
            &old.head,
        ],
        &old.root.join("data"),
        &old.root.join("config"),
    )?;
    assert!(result.status.success(), "{}", err(&result));
    assert!(
        out(&result).contains(&format!("backed up: 4 events, head {}", old.head)),
        "{}",
        out(&result)
    );
    assert_eq!(
        std::fs::metadata(&backup)?.permissions().mode() & 0o777,
        0o600
    );

    // A machine with nothing on it: no data directory, no config directory, no engine.pub.
    let fresh = machine("roundtrip-new")?;
    let (data, config) = (fresh.join("data"), fresh.join("config"));
    let result = hyalos(
        &[
            "log",
            "restore",
            b,
            "--public-key",
            &old.key_hex,
            "--expect-head",
            &old.head,
        ],
        &data,
        &config,
    )?;
    assert!(result.status.success(), "{}", err(&result));
    let text = out(&result);
    assert!(
        text.contains(&format!("restored: 4 events, head {}", old.head)),
        "{text}"
    );
    assert!(text.contains("no engine.pub on this machine yet"), "{text}");
    assert_eq!(
        std::fs::metadata(&data)?.permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(data.join("events.db"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    // The restored log verifies on its own, at the same head.
    let result = hyalos(
        &[
            "log",
            "verify",
            "--public-key",
            &old.key_hex,
            "--expect-head",
            &old.head,
        ],
        &data,
        &config,
    )?;
    assert!(result.status.success(), "{}", err(&result));
    assert!(out(&result).contains(&format!("ok: 4 events, head {}", old.head)));
    Ok(())
}

#[test]
fn restore_never_overwrites_a_log_that_is_there() -> Res {
    let old = old_machine("no-overwrite")?;
    let backup = old.root.join("backup.db");
    let b = backup.to_str().ok_or("path")?;
    let (data, config) = (old.root.join("data"), old.root.join("config"));
    assert!(
        hyalos(
            &["log", "backup", b, "--public-key", &old.key_hex],
            &data,
            &config
        )?
        .status
        .success()
    );
    let before = std::fs::read(data.join("events.db"))?;
    let result = hyalos(
        &["log", "restore", b, "--public-key", &old.key_hex],
        &data,
        &config,
    )?;
    assert_eq!(result.status.code(), Some(1));
    assert!(err(&result).contains("already exists"), "{}", err(&result));
    assert_eq!(std::fs::read(data.join("events.db"))?, before);
    // Nor does a second backup to the same name.
    let result = hyalos(
        &["log", "backup", b, "--public-key", &old.key_hex],
        &data,
        &config,
    )?;
    assert_eq!(result.status.code(), Some(1));
    assert!(err(&result).contains("already exists"));
    Ok(())
}

#[test]
fn a_backup_that_does_not_verify_is_not_restored_or_created() -> Res {
    let old = old_machine("bad")?;
    let (data, config) = (old.root.join("data"), old.root.join("config"));
    let backup = old.root.join("backup.db");
    let b = backup.to_str().ok_or("path")?;
    assert!(
        hyalos(
            &["log", "backup", b, "--public-key", &old.key_hex],
            &data,
            &config
        )?
        .status
        .success()
    );

    let fresh = machine("bad-new")?;
    let (fdata, fconfig) = (fresh.join("data"), fresh.join("config"));
    let wrong_key = LogKey::from_seed([1; 32]).public().to_hex();
    let wrong_head = format!("9:{}", "ab".repeat(32));
    let cases: [Vec<&str>; 2] = [
        // Not the key the log was made with.
        vec!["log", "restore", b, "--public-key", wrong_key.as_str()],
        // The right key, but the log does not end where the operator recorded.
        vec![
            "log",
            "restore",
            b,
            "--public-key",
            old.key_hex.as_str(),
            "--expect-head",
            wrong_head.as_str(),
        ],
    ];
    for args in cases {
        let result = hyalos(&args, &fdata, &fconfig)?;
        assert_eq!(result.status.code(), Some(1), "{args:?}: {}", err(&result));
        assert!(err(&result).starts_with("FAILED:"), "{}", err(&result));
        assert!(!fdata.join("events.db").exists() && !fdata.join("events.db.partial").exists());
    }

    // A tampered live log cannot be backed up as if it were good.
    {
        let conn = Connection::open(data.join("events.db"))?;
        conn.execute_batch("DROP TRIGGER events_no_update;")?;
        conn.execute("UPDATE events SET payload = x'7b7d7d' WHERE seq = 2", [])?;
    }
    let second = old.root.join("second.db");
    let result = hyalos(
        &[
            "log",
            "backup",
            second.to_str().ok_or("path")?,
            "--public-key",
            &old.key_hex,
        ],
        &data,
        &config,
    )?;
    assert_eq!(result.status.code(), Some(1));
    assert!(err(&result).contains("event 2"), "{}", err(&result));
    assert!(!second.exists());
    Ok(())
}

#[test]
fn mistakes_are_usage_errors() -> Res {
    let old = old_machine("usage")?;
    let (data, config) = (old.root.join("data"), old.root.join("config"));
    let key = old.key_hex.as_str();
    let cases: [&[&str]; 4] = [
        &["log", "backup", "--public-key", key],
        &["log", "restore", "--public-key", key],
        &["log", "restore", "a", "b", "--public-key", key],
        &["log", "verify", "stray", "--public-key", key],
    ];
    for args in cases {
        let result = hyalos(args, &data, &config)?;
        assert_eq!(result.status.code(), Some(2), "{args:?}: {}", err(&result));
        assert!(err(&result).contains("usage:"));
    }
    Ok(())
}
