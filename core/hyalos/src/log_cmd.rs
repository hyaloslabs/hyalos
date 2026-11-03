//! `hyalos log verify`, `backup` and `restore`: check, copy and recover an event log from the outside
//! (ADR-0018, ADR-0021).
//!
//! The public key comes from the operator, never from the log: from `--public-key`,
//! `--public-key-file`, or by default the `engine.pub` file the engine wrote when it created its key.
//! The database is opened read-only.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::ExitCode;

use hyalos_log::{Head, PublicKey, copy_verified, verify};

use crate::paths::{self, Env};

const USAGE: &str = "usage: hyalos log verify  [--db <path>] [KEY] [--expect-head <seq>:<hash>]
       hyalos log backup  <new-file> [--db <path>] [KEY] [--expect-head <seq>:<hash>]
       hyalos log restore <backup-file> [--db <path>] [KEY] [--expect-head <seq>:<hash>]
  KEY is --public-key <hex> or --public-key-file <path>
  --db is the live log: it defaults to events.db in the data directory; the key defaults to engine.pub in the
  config directory (HYALOS_DATA_DIR / HYALOS_CONFIG_DIR override both; see ADR-0018)
  backup copies the log (safe while the engine runs), checks the copy and only then creates <new-file>
  restore creates the live log from a backup; it never overwrites. On a fresh machine pass the original
  public key, since engine.pub is not there yet. Both refuse a copy that does not verify (ADR-0021)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Verify,
    Backup,
    Restore,
}

#[derive(Debug, PartialEq, Eq)]
struct Options {
    db: PathBuf,
    key: PublicKey,
    /// Where the key came from, shown with the result so the operator sees what was trusted.
    key_source: String,
    expect_head: Option<Head>,
    /// The file named on the command line: the new backup, or the backup to restore from.
    file: Option<PathBuf>,
}

pub fn run(args: &[String]) -> ExitCode {
    let command = match args.first().map(String::as_str) {
        Some("verify") => Command::Verify,
        Some("backup") => Command::Backup,
        Some("restore") => Command::Restore,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let env = |name: &str| std::env::var(name).ok();
    let options = match parse(command, args.get(1..).unwrap_or_default(), &env) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let outcome = match (command, &options.file) {
        (Command::Backup, Some(dest)) => {
            copy_verified(&options.db, dest, &options.key, options.expect_head)
                .map(|v| ("backed up", v, Some(dest.clone())))
                .map_err(|e| e.to_string())
        }
        (Command::Restore, Some(backup)) => {
            restore(backup, &options).map(|v| ("restored", v, Some(options.db.clone())))
        }
        _ => verify(&options.db, &options.key, options.expect_head)
            .map(|v| ("ok", v, None))
            .map_err(|e| e.to_string()),
    };
    match outcome {
        Ok((verb, v, to)) => {
            match v.head {
                Some(head) => println!("{verb}: {} events, head {head}", v.events),
                None => println!("{verb}: 0 events"),
            }
            if let Some(to) = to {
                println!("log at {}", to.display());
            }
            println!(
                "public key {} ({})",
                options.key.to_hex(),
                options.key_source
            );
            if command == Command::Restore {
                restore_note();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Creates the live log from a backup. The directory is made owner-only if it does not exist.
fn restore(backup: &std::path::Path, options: &Options) -> Result<hyalos_log::Verified, String> {
    if let Some(dir) = options.db.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    copy_verified(backup, &options.db, &options.key, options.expect_head).map_err(|e| e.to_string())
}

/// The engine needs the original signing key to carry on a restored log. Say so when there is no
/// `engine.pub` to go with it.
fn restore_note() {
    let have_public_key = paths::engine_public_key(&|name: &str| std::env::var(name).ok())
        .map(|p| p.exists())
        .unwrap_or(false);
    if !have_public_key {
        println!(
            "note: there is no engine.pub on this machine yet. Checking this log used the key you gave; \
             the engine needs the original signing key to add to it (story S1.8)."
        );
    }
}

fn parse(command: Command, args: &[String], env: Env<'_>) -> Result<Options, String> {
    let mut db = None;
    let mut key_hex = None;
    let mut key_file = None;
    let mut expect = None;
    let mut files: Vec<&String> = Vec::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--db" => db = Some(value()?),
            "--public-key" => key_hex = Some(value()?),
            "--public-key-file" => key_file = Some(value()?),
            "--expect-head" => expect = Some(value()?),
            other if other.starts_with('-') => return Err(format!("unknown argument {other}")),
            _ => files.push(flag),
        }
    }
    if key_hex.is_some() && key_file.is_some() {
        return Err("give --public-key or --public-key-file, not both".into());
    }
    let file = match (command, files.as_slice()) {
        (Command::Verify, []) => None,
        (Command::Verify, [extra, ..]) => return Err(format!("unexpected argument {extra}")),
        (Command::Backup, [one]) => Some(PathBuf::from(one)),
        (Command::Restore, [one]) => Some(PathBuf::from(one)),
        (Command::Backup, []) => return Err("backup needs the name of the new file".into()),
        (Command::Restore, []) => return Err("restore needs the backup file to restore".into()),
        (_, [_, extra, ..]) => return Err(format!("unexpected argument {extra}")),
    };

    let db = match db {
        Some(path) => PathBuf::from(path),
        None => paths::log_file(env)?,
    };
    let (key_text, key_source) = match (key_hex, key_file) {
        (Some(hex), _) => (hex, "from --public-key".to_string()),
        (None, Some(path)) => (read_key_file(&path)?, format!("from {path}")),
        (None, None) => {
            let path = paths::engine_public_key(env)?;
            let shown = path.display().to_string();
            if !path.exists() {
                return Err(format!(
                    "no public key was given and there is none at {shown}; pass --public-key or --public-key-file"
                ));
            }
            // The default file is trusted without being asked for, so refuse one that other users could
            // have replaced. A file named with --public-key-file is the operator's own choice.
            let mode = std::fs::metadata(&path)
                .map_err(|e| format!("cannot read {shown}: {e}"))?
                .permissions()
                .mode();
            if mode & 0o022 != 0 {
                return Err(format!(
                    "{shown} can be written by other users (mode {:04o}), so it cannot be trusted; run chmod go-w on it, or pass --public-key or --public-key-file",
                    mode & 0o7777
                ));
            }
            (read_key_file(&shown)?, format!("from {shown}"))
        }
    };
    let key = PublicKey::from_hex(&key_text)
        .ok_or("the public key must be 64 hexadecimal characters (32 bytes)")?;
    let expect_head = match expect {
        None => None,
        Some(text) => Some(
            Head::parse(&text).ok_or("--expect-head must look like <seq>:<64 hex characters>")?,
        ),
    };
    Ok(Options {
        db,
        key,
        key_source,
        expect_head,
        file,
    })
}

fn read_key_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the public key file {path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    const KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    /// An environment with no home and no overrides.
    fn nowhere(_: &str) -> Option<String> {
        None
    }

    /// An environment that puts both directories under `dir`.
    fn at(dir: &'static str) -> impl Fn(&str) -> Option<String> {
        move |name| match name {
            "HYALOS_DATA_DIR" => Some(format!("{dir}/data")),
            "HYALOS_CONFIG_DIR" => Some(format!("{dir}/config")),
            _ => None,
        }
    }

    #[test]
    fn explicit_flags_need_no_directories() {
        let o = parse(
            Command::Verify,
            &args(&["--db", "a.db", "--public-key", KEY]),
            &nowhere,
        )
        .unwrap();
        assert_eq!(o.db, PathBuf::from("a.db"));
        assert_eq!(o.key, PublicKey::from_bytes([1; 32]));
        assert_eq!(o.key_source, "from --public-key");
        assert_eq!(o.expect_head, None);
    }

    #[test]
    fn the_database_defaults_to_the_data_directory() {
        let o = parse(Command::Verify, &args(&["--public-key", KEY]), &at("/h")).unwrap();
        assert_eq!(o.db, PathBuf::from("/h/data/events.db"));
    }

    #[test]
    fn a_missing_default_key_file_says_what_to_do() {
        let err = parse(
            Command::Verify,
            &args(&["--db", "a.db"]),
            &at("/definitely/not/here"),
        )
        .unwrap_err();
        assert!(
            err.contains("none at /definitely/not/here/config/engine.pub"),
            "{err}"
        );
        assert!(err.contains("--public-key or --public-key-file"), "{err}");
    }

    #[test]
    fn a_directory_is_looked_up_only_when_its_default_is_needed() {
        let err = parse(Command::Verify, &args(&["--public-key", KEY]), &nowhere).unwrap_err();
        assert!(err.contains("set HYALOS_DATA_DIR"), "{err}");
        assert!(
            parse(
                Command::Verify,
                &args(&["--db", "a.db", "--public-key", KEY]),
                &nowhere
            )
            .is_ok()
        );
        // An explicit key file does not need the config directory, and the default log does not need it.
        let only_data = |name: &str| (name == "HYALOS_DATA_DIR").then(|| "/d".to_string());
        let o = parse(Command::Verify, &args(&["--public-key", KEY]), &only_data).unwrap();
        assert_eq!(o.db, PathBuf::from("/d/events.db"));
    }

    #[test]
    fn backup_and_restore_take_exactly_one_file() {
        let o = parse(
            Command::Backup,
            &args(&["new.db", "--db", "live.db", "--public-key", KEY]),
            &nowhere,
        )
        .unwrap();
        assert_eq!(
            (o.db, o.file),
            (PathBuf::from("live.db"), Some(PathBuf::from("new.db")))
        );
        let o = parse(
            Command::Restore,
            &args(&["b.db", "--public-key", KEY]),
            &at("/h"),
        )
        .unwrap();
        assert_eq!(
            o.db,
            PathBuf::from("/h/data/events.db"),
            "the live log is the default target"
        );
        assert_eq!(o.file, Some(PathBuf::from("b.db")));

        let cases: [(Command, &[&str], &str); 5] = [
            (
                Command::Backup,
                &["--db", "a.db", "--public-key", KEY],
                "name of the new file",
            ),
            (
                Command::Restore,
                &["--db", "a.db", "--public-key", KEY],
                "backup file to restore",
            ),
            (
                Command::Backup,
                &["a", "b", "--db", "c", "--public-key", KEY],
                "unexpected argument b",
            ),
            (
                Command::Restore,
                &["a", "b", "--db", "c", "--public-key", KEY],
                "unexpected argument b",
            ),
            (
                Command::Verify,
                &["stray", "--db", "c", "--public-key", KEY],
                "unexpected argument stray",
            ),
        ];
        for (command, list, expected) in cases {
            let err = parse(command, &args(list), &nowhere).unwrap_err();
            assert!(err.contains(expected), "{command:?} {list:?}: {err}");
        }
    }

    #[test]
    fn a_head_is_parsed() {
        let head = format!("7:{}", "ab".repeat(32));
        let o = parse(
            Command::Verify,
            &args(&["--db", "a.db", "--public-key", KEY, "--expect-head", &head]),
            &nowhere,
        )
        .unwrap();
        assert_eq!(o.expect_head.map(|h| h.seq), Some(7));
    }

    #[test]
    fn mistakes_are_refused_with_a_reason() {
        let cases: [(&[&str], &str); 5] = [
            (&["--db", "a.db", "--public-key", "abc"], "64 hexadecimal"),
            (
                &[
                    "--db",
                    "a.db",
                    "--public-key",
                    KEY,
                    "--public-key-file",
                    "k",
                ],
                "not both",
            ),
            (
                &["--db", "a.db", "--public-key", KEY, "--expect-head", "7"],
                "--expect-head must",
            ),
            (&["--db"], "needs a value"),
            (&["--db", "a.db", "--bogus"], "unknown argument --bogus"),
        ];
        for (list, expected) in cases {
            let err = parse(Command::Verify, &args(list), &nowhere).unwrap_err();
            assert!(err.contains(expected), "{list:?}: {err}");
        }
    }
}
