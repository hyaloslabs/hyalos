//! `hyalos log verify` against the real binary (S1.1: tampering fails verification naming the event).

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hyalos_log::{Log, LogKey, Origin};
use hyalos_types::Timestamp;
use rusqlite::Connection;

type Res<T = ()> = Result<T, Box<dyn Error>>;

struct Fixture {
    dir: PathBuf,
    db: PathBuf,
    key_hex: String,
    head: String,
}

fn fixture(name: &str) -> Res<Fixture> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("events.db");
    let key = LogKey::from_seed([9; 32]);
    let key_hex = key.public().to_hex();
    let mut log = Log::open(&db, key)?;
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
    Ok(Fixture {
        dir,
        db,
        key_hex,
        head,
    })
}

/// Runs the binary with none of the host's Hyalos or XDG settings, so a test sees only what it sets.
fn hyalos_with(args: &[&str], env: &[(&str, &Path)]) -> Res<Output> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hyalos"));
    cmd.args(args)
        .env_remove("HYALOS_DATA_DIR")
        .env_remove("HYALOS_CONFIG_DIR")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME");
    for (k, v) in env {
        cmd.env(k, v);
    }
    Ok(cmd.output()?)
}

fn hyalos(args: &[&str]) -> Res<Output> {
    hyalos_with(args, &[])
}

fn verify(f: &Fixture, extra: &[&str]) -> Res<Output> {
    let db = f.db.to_str().ok_or("db path is not utf-8")?;
    let mut args = vec!["log", "verify", "--db", db];
    args.extend_from_slice(extra);
    hyalos(&args)
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn a_good_log_verifies_and_prints_its_head() -> Res {
    let f = fixture("good")?;
    let o = verify(&f, &["--public-key", &f.key_hex])?;
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(format!("ok: 4 events, head {}", f.head).as_str())
    );
    // The result names the key it trusted and where that came from.
    assert_eq!(
        lines.next(),
        Some(format!("public key {} (from --public-key)", f.key_hex).as_str())
    );
    Ok(())
}

#[test]
fn the_key_can_come_from_a_file() -> Res {
    let f = fixture("keyfile")?;
    let file = f.dir.join("engine.pub");
    std::fs::write(&file, format!("{}\n", f.key_hex))?;
    let o = verify(&f, &["--public-key-file", file.to_str().ok_or("path")?])?;
    assert!(o.status.success(), "{}", err(&o));
    Ok(())
}

#[test]
fn a_tampered_event_fails_and_is_named() -> Res {
    let f = fixture("tampered")?;
    let conn = Connection::open(&f.db)?;
    conn.execute_batch("DROP TRIGGER events_no_update;")?;
    conn.execute(
        "UPDATE events SET payload = x'7b2268223a317d' WHERE seq = 2",
        [],
    )?;
    drop(conn);
    let o = verify(&f, &["--public-key", &f.key_hex])?;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("FAILED: event 2:"), "{}", err(&o));
    assert!(out(&o).is_empty());
    Ok(())
}

#[test]
fn the_wrong_public_key_fails() -> Res {
    let f = fixture("wrongkey")?;
    let other = LogKey::from_seed([1; 32]).public().to_hex();
    let o = verify(&f, &["--public-key", &other])?;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("event 1:"), "{}", err(&o));
    Ok(())
}

#[test]
fn a_recorded_head_catches_removed_events() -> Res {
    let f = fixture("head")?;
    let good = verify(&f, &["--public-key", &f.key_hex, "--expect-head", &f.head])?;
    assert!(good.status.success(), "{}", err(&good));

    let conn = Connection::open(&f.db)?;
    conn.execute_batch("DROP TRIGGER events_no_delete;")?;
    conn.execute("DELETE FROM events WHERE seq = 4", [])?;
    drop(conn);
    let o = verify(&f, &["--public-key", &f.key_hex, "--expect-head", &f.head])?;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("expected head"), "{}", err(&o));
    // Without the recorded head, the shortened log still looks fine. That is the known limit.
    let blind = verify(&f, &["--public-key", &f.key_hex])?;
    assert!(blind.status.success());
    Ok(())
}

#[test]
fn usage_mistakes_exit_2_and_never_pass() -> Res {
    let f = fixture("usage")?;
    for extra in [&[][..], &["--public-key", "abc"][..]] {
        let o = verify(&f, extra)?;
        assert_eq!(o.status.code(), Some(2), "{extra:?}");
        assert!(out(&o).is_empty());
    }
    assert_eq!(hyalos(&["log"])?.status.code(), Some(2));
    assert_eq!(hyalos(&["bogus"])?.status.code(), Some(2));
    Ok(())
}

#[test]
fn a_missing_log_is_a_failure_not_a_pass() -> Res {
    let f = fixture("missing")?;
    let o = hyalos(&[
        "log",
        "verify",
        "--db",
        f.dir.join("nope.db").to_str().ok_or("path")?,
        "--public-key",
        &f.key_hex,
    ])?;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("FAILED: cannot read the event log"),
        "{}",
        err(&o)
    );
    Ok(())
}

#[test]
fn the_version_still_prints() -> Res {
    let o = hyalos(&["--version"])?;
    assert!(out(&o).starts_with("hyalos "));
    Ok(())
}

/// Lays out a data and a config directory the way the engine will, with the log and `engine.pub`.
fn installed(name: &str, pub_hex: &str) -> Res<(PathBuf, PathBuf, Fixture)> {
    let f = fixture(name)?;
    let data = f.dir.join("data");
    let config = f.dir.join("config");
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(&config)?;
    std::fs::rename(&f.db, data.join("events.db"))?;
    std::fs::write(config.join("engine.pub"), format!("{pub_hex}\n"))?;
    Ok((data, config, f))
}

fn good_key() -> String {
    LogKey::from_seed([9; 32]).public().to_hex()
}

#[test]
fn with_the_engine_files_in_place_verify_needs_no_arguments() -> Res {
    let (data, config, _f) = installed("defaults", &good_key())?;
    let env = [
        ("HYALOS_DATA_DIR", data.as_path()),
        ("HYALOS_CONFIG_DIR", config.as_path()),
    ];
    let o = hyalos_with(&["log", "verify"], &env)?;
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).starts_with("ok: 4 events"), "{}", out(&o));
    let key_file = config.join("engine.pub");
    assert!(
        out(&o).contains(&format!("(from {})", key_file.display())),
        "{}",
        out(&o)
    );
    Ok(())
}

#[test]
fn the_default_locations_follow_xdg_when_no_override_is_set() -> Res {
    let (data, config, f) = installed("xdg", &good_key())?;
    // XDG_*_HOME name the parents; Hyalos adds `hyalos`.
    let root = f.dir.join("xdg-root");
    std::fs::create_dir_all(root.join("data"))?;
    std::fs::create_dir_all(root.join("config"))?;
    std::fs::rename(&data, root.join("data/hyalos"))?;
    std::fs::rename(&config, root.join("config/hyalos"))?;
    let (xdg_data, xdg_config) = (root.join("data"), root.join("config"));
    let env = [
        ("XDG_DATA_HOME", xdg_data.as_path()),
        ("XDG_CONFIG_HOME", xdg_config.as_path()),
    ];
    let o = hyalos_with(&["log", "verify"], &env)?;
    assert!(o.status.success(), "{}", err(&o));
    Ok(())
}

#[test]
fn an_engine_pub_holding_a_different_key_fails_the_log() -> Res {
    // The trusted key file decides the answer, which is why it must be protected (ADR-0018). This shows
    // the check uses that file and nothing inside the log.
    let other = LogKey::from_seed([1; 32]).public().to_hex();
    let (data, config, _f) = installed("otherkey", &other)?;
    let env = [
        ("HYALOS_DATA_DIR", data.as_path()),
        ("HYALOS_CONFIG_DIR", config.as_path()),
    ];
    let o = hyalos_with(&["log", "verify"], &env)?;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("event 1:"), "{}", err(&o));
    Ok(())
}

#[test]
fn a_flag_overrides_the_default_key_file() -> Res {
    let wrong = LogKey::from_seed([1; 32]).public().to_hex();
    let (data, config, f) = installed("override", &wrong)?;
    let env = [
        ("HYALOS_DATA_DIR", data.as_path()),
        ("HYALOS_CONFIG_DIR", config.as_path()),
    ];
    let o = hyalos_with(&["log", "verify", "--public-key", &f.key_hex], &env)?;
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("(from --public-key)"), "{}", out(&o));
    Ok(())
}

#[test]
fn a_missing_engine_pub_is_a_usage_error_that_says_what_to_do() -> Res {
    let (data, config, _f) = installed("nokey", &good_key())?;
    std::fs::remove_file(config.join("engine.pub"))?;
    let env = [
        ("HYALOS_DATA_DIR", data.as_path()),
        ("HYALOS_CONFIG_DIR", config.as_path()),
    ];
    let o = hyalos_with(&["log", "verify"], &env)?;
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("--public-key or --public-key-file"),
        "{}",
        err(&o)
    );
    assert!(out(&o).is_empty());
    Ok(())
}

#[test]
fn without_any_home_the_error_names_the_variable_to_set() -> Res {
    let o = hyalos(&["log", "verify"])?;
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("set HYALOS_DATA_DIR"), "{}", err(&o));
    Ok(())
}
