//! `hyalos init` against the real binary (S1.8, ADR-0022).

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hyalos_log::{Log, LogKey, Origin, format};
use hyalos_types::Timestamp;

type Res<T = ()> = Result<T, Box<dyn Error>>;

struct Host {
    data: PathBuf,
    config: PathBuf,
}

fn host(name: &str) -> Res<Host> {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-init-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    // Neither directory exists yet: creating them is part of first start.
    Ok(Host {
        data: root.join("data"),
        config: root.join("config"),
    })
}

fn run(h: &Host, args: &[&str]) -> Res<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_hyalos"))
        .args(args)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME")
        .env("HYALOS_DATA_DIR", &h.data)
        .env("HYALOS_CONFIG_DIR", &h.config)
        .output()?)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn mode(p: &Path) -> Res<u32> {
    Ok(std::fs::metadata(p)?.permissions().mode() & 0o777)
}

/// The secret in `engine.key`, read the way only a test should.
fn seed_hex(h: &Host) -> Res<String> {
    let raw = std::fs::read_to_string(h.config.join("engine.key"))?;
    Ok(raw
        .trim_end()
        .strip_prefix("hyalos-engine-key-v1:")
        .ok_or("unexpected key file")?
        .to_string())
}

fn key_of(h: &Host) -> Res<LogKey> {
    let seed = format::from_hex::<32>(&seed_hex(h)?).ok_or("bad seed")?;
    Ok(LogKey::from_seed(seed))
}

fn append(h: &Host, n: u64) -> Res<String> {
    let mut log = Log::open(&h.data.join("events.db"), key_of(h)?)?;
    let mut head = String::new();
    for i in 1..=n {
        head = log
            .append(Timestamp::from_millis(i), Origin::Engine, "test", 1, b"{}")?
            .to_string();
    }
    Ok(head)
}

#[test]
fn first_start_makes_the_key_and_verify_then_needs_no_arguments() -> Res {
    let h = host("first")?;
    let out = run(&h, &["init"])?;
    assert!(out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(
        shown.contains("created the engine's signing key"),
        "{shown}"
    );
    assert!(
        shown.contains("Back up engine.key"),
        "the operator is told to back the key up: {shown}"
    );

    assert_eq!(mode(&h.config)?, 0o700);
    assert_eq!(mode(&h.data)?, 0o700);
    assert_eq!(mode(&h.config.join("engine.key"))?, 0o600);
    assert_eq!(mode(&h.config.join("engine.pub"))?, 0o444);
    let public = std::fs::read_to_string(h.config.join("engine.pub"))?;
    assert_eq!(public.trim(), key_of(&h)?.public().to_hex());
    assert!(
        shown.contains(&format!("public key {}", public.trim())),
        "{shown}"
    );

    // Once there is a log, verify finds log and key by itself (ADR-0018).
    let head = append(&h, 4)?;
    let v = run(&h, &["log", "verify"])?;
    assert!(v.status.success(), "{}", text(&v));
    let verified = text(&v);
    assert!(
        verified.contains(&format!("ok: 4 events, head {head}")),
        "{verified}"
    );
    assert!(
        verified.contains(&format!("public key {}", public.trim())),
        "{verified}"
    );
    Ok(())
}

#[test]
fn running_init_again_changes_nothing() -> Res {
    let h = host("again")?;
    assert!(run(&h, &["init"])?.status.success());
    let before = (
        std::fs::read(h.config.join("engine.key"))?,
        std::fs::read(h.config.join("engine.pub"))?,
    );
    let second = run(&h, &["init"])?;
    assert!(second.status.success(), "{}", text(&second));
    assert!(
        text(&second).contains("engine key ok: public key"),
        "{}",
        text(&second)
    );
    assert_eq!(
        (
            std::fs::read(h.config.join("engine.key"))?,
            std::fs::read(h.config.join("engine.pub"))?
        ),
        before
    );
    Ok(())
}

#[test]
fn nothing_the_binary_prints_or_writes_contains_the_key() -> Res {
    let h = host("no-leak")?;
    let mut shown = text(&run(&h, &["init"])?);
    append(&h, 6)?;
    shown += &text(&run(&h, &["init"])?);
    shown += &text(&run(&h, &["log", "verify"])?);
    let backup = h.config.join("..").join("backup.db");
    shown += &text(&run(
        &h,
        &["log", "backup", backup.to_str().ok_or("path")?],
    )?);
    // Failures print too: a bad mode, and a lost key.
    std::fs::set_permissions(
        h.config.join("engine.key"),
        std::fs::Permissions::from_mode(0o644),
    )?;
    shown += &text(&run(&h, &["init"])?);

    let secret = seed_hex(&h)?;
    assert!(!shown.contains(&secret), "the key was printed");
    let seed = format::from_hex::<32>(&secret).ok_or("seed")?;
    for name in ["events.db", "events.db-wal", "events.db-shm"] {
        if let Ok(bytes) = std::fs::read(h.data.join(name)) {
            assert!(
                !bytes.windows(32).any(|w| w == seed),
                "{name} holds the raw key"
            );
            assert!(
                !bytes.windows(64).any(|w| w == secret.as_bytes()),
                "{name} holds the key as hex"
            );
        }
    }
    let copy = std::fs::read(&backup)?;
    assert!(
        !copy.windows(32).any(|w| w == seed) && !copy.windows(64).any(|w| w == secret.as_bytes())
    );
    Ok(())
}

#[test]
fn a_lost_key_is_refused_and_the_old_log_still_verifies() -> Res {
    let h = host("lost")?;
    assert!(run(&h, &["init"])?.status.success());
    append(&h, 3)?;
    std::fs::remove_file(h.config.join("engine.key"))?;

    let out = run(&h, &["init"])?;
    assert_eq!(out.status.code(), Some(1));
    let shown = text(&out);
    assert!(shown.contains("will not start a new key"), "{shown}");
    assert!(
        shown.contains("move") && shown.contains("engine.pub"),
        "{shown}"
    );
    assert!(
        !h.config.join("engine.key").exists(),
        "no replacement key may appear"
    );

    // The public key that was kept still verifies everything that was written.
    let v = run(&h, &["log", "verify"])?;
    assert!(v.status.success(), "{}", text(&v));
    assert!(text(&v).contains("ok: 3 events"));
    Ok(())
}

#[test]
fn a_key_others_can_read_is_refused() -> Res {
    let h = host("mode")?;
    assert!(run(&h, &["init"])?.status.success());
    std::fs::set_permissions(
        h.config.join("engine.key"),
        std::fs::Permissions::from_mode(0o644),
    )?;
    let out = run(&h, &["init"])?;
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("chmod 600"), "{}", text(&out));
    Ok(())
}

#[test]
fn backup_restore_and_the_key_on_another_machine_let_the_engine_carry_on() -> Res {
    let old = host("restore-old")?;
    assert!(run(&old, &["init"])?.status.success());
    let head = append(&old, 5)?;
    let backup = old.config.join("../backup.db");
    let b = backup.to_str().ok_or("path")?;
    let public = key_of(&old)?.public().to_hex();
    assert!(
        run(&old, &["log", "backup", b, "--expect-head", &head])?
            .status
            .success()
    );

    // A machine with nothing on it: restore, then see that the log cannot yet be extended.
    let new = host("restore-new")?;
    let r = run(
        &new,
        &[
            "log",
            "restore",
            b,
            "--public-key",
            &public,
            "--expect-head",
            &head,
        ],
    )?;
    assert!(r.status.success(), "{}", text(&r));
    let frozen = run(&new, &["init"])?;
    assert_eq!(frozen.status.code(), Some(1));
    assert!(
        text(&frozen).contains("will not start a new key"),
        "{}",
        text(&frozen)
    );

    // Carry the key across and run init: engine.pub is written, and verify needs no arguments.
    std::fs::create_dir_all(&new.config)?;
    std::fs::set_permissions(&new.config, std::fs::Permissions::from_mode(0o700))?;
    std::fs::copy(old.config.join("engine.key"), new.config.join("engine.key"))?;
    std::fs::set_permissions(
        new.config.join("engine.key"),
        std::fs::Permissions::from_mode(0o600),
    )?;
    let init = run(&new, &["init"])?;
    assert!(init.status.success(), "{}", text(&init));
    assert!(text(&init).contains("wrote"), "{}", text(&init));
    let v = run(&new, &["log", "verify", "--expect-head", &head])?;
    assert!(v.status.success(), "{}", text(&v));
    assert!(text(&v).contains(&public));

    // The restored log can be extended with the carried key, and the longer log verifies.
    let mut log = Log::open(&new.data.join("events.db"), key_of(&new)?)?;
    let next = log.append(Timestamp::from_millis(99), Origin::Engine, "test", 1, b"{}")?;
    assert_eq!(next.seq, 6);
    let v = run(&new, &["log", "verify", "--expect-head", &next.to_string()])?;
    assert!(v.status.success(), "{}", text(&v));
    Ok(())
}

#[test]
fn init_takes_no_arguments() -> Res {
    let h = host("usage")?;
    let out = run(&h, &["init", "stray"])?;
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out).contains("usage: hyalos init"));
    assert!(!h.config.exists(), "a usage error creates nothing");
    Ok(())
}
