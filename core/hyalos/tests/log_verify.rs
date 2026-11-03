//! `hyalos log verify` against the real binary (S1.1: tampering fails verification naming the event).

use std::error::Error;
use std::path::PathBuf;
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

fn hyalos(args: &[&str]) -> Res<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_hyalos"))
        .args(args)
        .output()?)
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
    assert_eq!(out(&o).trim(), format!("ok: 4 events, head {}", f.head));
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
