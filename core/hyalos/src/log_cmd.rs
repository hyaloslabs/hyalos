//! `hyalos log verify`, `backup`, `restore`, `compact` and `prune`: check, copy, recover and shrink an
//! event log from the outside (ADR-0018, ADR-0021, ADR-0026).
//!
//! The public key comes from the operator, never from the log: from `--public-key`,
//! `--public-key-file`, or by default the `engine.pub` file the engine wrote when it created its key.
//! Checking opens the database read-only. `compact` is the one command that writes to it, so it needs
//! the engine's own key. `prune` is the one that deletes anything: it needs `--yes`, and it records the
//! prune in the log before it deletes, so it needs the engine's key as well.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::ExitCode;

use hyalos_engine::Engine;
use hyalos_log::{Head, PublicKey, Verified, copy_verified, verify, verify_history};
use hyalos_types::Timestamp;

use crate::keys;
use crate::paths::{self, Env};

const USAGE: &str = "usage: hyalos log verify  [--db <path>] [KEY] [--expect-head <seq>:<hash>] [--archive <file>]...
       hyalos log backup  <new-file> [--db <path>] [KEY] [--expect-head <seq>:<hash>]
       hyalos log restore <backup-file> [--db <path>] [KEY] [--expect-head <seq>:<hash>]
       hyalos log compact [--db <path>] [--archive-dir <dir>] [--key-file <path>]
       hyalos log prune   <archive> --yes [--db <path>] [--key-file <path>]
  KEY is --public-key <hex> or --public-key-file <path>
  --db is the live log: it defaults to events.db in the data directory; the key defaults to engine.pub in the
  config directory (HYALOS_DATA_DIR / HYALOS_CONFIG_DIR override both; see ADR-0018)
  backup copies the log (safe while the engine runs), checks the copy and only then creates <new-file>
  restore creates the live log from a backup; it never overwrites. On a fresh machine pass the original
  public key, since engine.pub is not there yet. Both refuse a copy that does not verify (ADR-0021)
  compact moves every event to a new archive file and leaves a signed snapshot of the state in the log
  (ADR-0026). Nothing is deleted and no hash changes. It needs the engine's signing key (default
  engine.key in the config directory) and the engine stopped (if the log changes while it runs it refuses
  and changes nothing); archives go to the 'archive' directory beside the log unless --archive-dir says
  otherwise
  verify --archive checks an archive too, and that it ends where a snapshot of this log stands; without
  it the history before the latest snapshot rests on the engine's signature on that snapshot
  prune deletes one archive, after checking it belongs to this log, and first records that in the log
  (an archive.pruned event), so it needs the engine's signing key like compact and the engine stopped. It
  is the only command that removes history from this machine, so it asks for --yes; save the archive
  somewhere first. verify lists the archives that were pruned";

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
    /// The file named on the command line: the new backup or the backup to restore from.
    file: Option<PathBuf>,
    /// Archives to check along with the log (`verify --archive`).
    archives: Vec<PathBuf>,
}

/// What a command did, for printing.
struct Done {
    verb: &'static str,
    verified: Verified,
    /// The file the command made or removed, shown after the result.
    to: Option<PathBuf>,
    notes: Vec<String>,
}

pub fn run(args: &[String]) -> ExitCode {
    let command = match args.first().map(String::as_str) {
        Some("verify") => Command::Verify,
        Some("backup") => Command::Backup,
        Some("restore") => Command::Restore,
        Some("compact") => return compact(args.get(1..).unwrap_or_default()),
        Some("prune") => return prune(args.get(1..).unwrap_or_default()),
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
                .map(|v| Done {
                    verb: "backed up",
                    verified: v,
                    to: Some(dest.clone()),
                    notes: Vec::new(),
                })
                .map_err(|e| e.to_string())
        }
        (Command::Restore, Some(backup)) => restore(backup, &options).map(|v| Done {
            verb: "restored",
            verified: v,
            to: Some(options.db.clone()),
            notes: Vec::new(),
        }),
        _ => check(&options),
    };
    match outcome {
        Ok(done) => {
            let v = done.verified;
            let verb = done.verb;
            match (v.head, v.anchored) {
                (Some(head), Some(snapshot)) => println!(
                    "{verb}: {} events after the snapshot at {snapshot}, head {head}",
                    v.events
                ),
                (Some(head), None) => println!("{verb}: {} events, head {head}", v.events),
                (None, _) => println!("{verb}: 0 events"),
            }
            if let Some(to) = done.to {
                println!("log at {}", to.display());
            }
            for note in done.notes {
                println!("{note}");
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

/// `verify`, together with any archives named.
fn check(options: &Options) -> Result<Done, String> {
    let history = verify_history(
        &options.db,
        &options.archives,
        &options.key,
        options.expect_head,
    )
    .map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    if let Some(snapshot) = history.live.anchored {
        notes.push(if history.from_genesis {
            format!(
                "history from event 1 verified, through {} archive(s)",
                history.archives
            )
        } else if history.archives == 0 {
            format!(
                "events up to {} were compacted; they rest on the engine's signature on the snapshot. Give --archive <file> for each archive to check them too",
                snapshot.seq
            )
        } else {
            format!(
                "events up to {} were compacted and the archives given do not cover all of them; the rest rest on the engine's signature on the snapshot",
                snapshot.seq
            )
        });
    }
    for p in hyalos_engine::pruned_archives(&options.db, &options.key).map_err(|e| e.to_string())? {
        notes.push(format!(
            "pruned: the archive ending at event {} ({}) was removed, recorded at event {}, unix time {}",
            p.last_seq,
            p.last_hash,
            p.seq,
            p.at.as_millis() / 1000
        ));
    }
    Ok(Done {
        verb: "ok",
        verified: history.live,
        to: None,
        notes,
    })
}

/// `hyalos log prune`: delete one archive, once it is shown to be part of this log's history, and
/// only after the log says so. It never deletes anything that does not check out, and never the log
/// itself. Like `compact` it signs, so it needs the engine's key and the engine stopped.
fn prune(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let options = match parse_prune(args, &env) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run_prune(&options) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PruneOptions {
    db: PathBuf,
    key_file: PathBuf,
    archive: PathBuf,
}

fn parse_prune(args: &[String], env: Env<'_>) -> Result<PruneOptions, String> {
    let (mut db, mut key_file, mut yes) = (None, None, false);
    let mut files: Vec<&String> = Vec::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--db" => db = Some(PathBuf::from(value()?)),
            "--key-file" => key_file = Some(PathBuf::from(value()?)),
            "--yes" => yes = true,
            other if other.starts_with('-') => return Err(format!("unknown argument {other}")),
            _ => files.push(flag),
        }
    }
    let archive = match files.as_slice() {
        [one] => PathBuf::from(one),
        [] => return Err("prune needs the archive to remove".into()),
        [_, extra, ..] => return Err(format!("unexpected argument {extra}")),
    };
    if !yes {
        return Err(
            "prune removes history from this machine for good; save the archive somewhere first, then pass --yes"
                .into(),
        );
    }
    let db = match db {
        Some(db) => db,
        None => paths::log_file(env)?,
    };
    let key_file = match key_file {
        Some(path) => path,
        None => paths::config_dir(env)?.join(paths::ENGINE_KEY_FILE),
    };
    Ok(PruneOptions {
        db,
        key_file,
        archive,
    })
}

fn run_prune(options: &PruneOptions) -> Result<Vec<String>, String> {
    let archive = &options.archive;
    let key = keys::load(&options.key_file).map_err(|e| e.to_string())?;
    let public = key.public();
    // The archive must verify on its own and hold events. That it ends where a snapshot of this log
    // stands is checked by the engine when it records the prune, before anything is deleted.
    let what = verify(archive, &public, None).map_err(|e| format!("{}: {e}", archive.display()))?;
    let head = what
        .head
        .filter(|_| what.events > 0)
        .ok_or_else(|| format!("{} holds no events", archive.display()))?;

    // Recorded first. A crash after this leaves a record and a file that can be pruned again; the
    // other order could leave a deleted archive and nothing to say so.
    let mut engine = Engine::open(&options.db, key).map_err(|e| e.to_string())?;
    let note = if engine.is_pruned(head) {
        "already recorded in the log; removing the file".to_string()
    } else {
        let recorded = engine
            .record_prune(now()?, head)
            .map_err(|e| e.to_string())?;
        format!("recorded in the log at event {}", recorded.seq)
    };
    std::fs::remove_file(archive)
        .map_err(|e| format!("cannot remove {}: {e}", archive.display()))?;
    Ok(vec![
        format!(
            "pruned: {} ({} events, ending at {head})",
            archive.display(),
            what.events
        ),
        note,
        "that history is no longer on this machine; the snapshot that stands for it stays in the log"
            .into(),
        format!("public key {}", public.to_hex()),
    ])
}

fn now() -> Result<Timestamp, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("the system clock is before 1970: {e}"))?;
    Ok(Timestamp::from_millis(
        u64::try_from(now.as_millis()).unwrap_or(u64::MAX),
    ))
}

/// `hyalos log compact`: archive every event and leave a snapshot. It needs the engine's own key, since
/// it signs the snapshot, and the engine stopped, since it is the log's only writer.
fn compact(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let options = match parse_compact(args, &env) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run_compact(&options) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CompactOptions {
    db: PathBuf,
    key_file: PathBuf,
    archive_dir: PathBuf,
}

fn parse_compact(args: &[String], env: Env<'_>) -> Result<CompactOptions, String> {
    let (mut db, mut key_file, mut archive_dir) = (None, None, None);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--db" => db = Some(PathBuf::from(value()?)),
            "--key-file" => key_file = Some(PathBuf::from(value()?)),
            "--archive-dir" => archive_dir = Some(PathBuf::from(value()?)),
            other if other.starts_with('-') => return Err(format!("unknown argument {other}")),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let db = match db {
        Some(db) => db,
        None => paths::log_file(env)?,
    };
    let key_file = match key_file {
        Some(path) => path,
        None => paths::config_dir(env)?.join(paths::ENGINE_KEY_FILE),
    };
    // Beside the log it came from, so a backup of the data directory carries its archives.
    let archive_dir = archive_dir.unwrap_or_else(|| {
        db.parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("archive")
    });
    Ok(CompactOptions {
        db,
        key_file,
        archive_dir,
    })
}

fn run_compact(options: &CompactOptions) -> Result<Vec<String>, String> {
    let key = keys::load(&options.key_file).map_err(|e| e.to_string())?;
    let public = key.public();
    let mut engine = Engine::open(&options.db, key).map_err(|e| e.to_string())?;
    let first = engine
        .snapshot_head()
        .map_err(|e| e.to_string())?
        .map_or(1, |h| h.seq + 1);
    let last = engine
        .head()
        .map_err(|e| e.to_string())?
        .map_or(0, |h| h.seq);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&options.archive_dir)
        .map_err(|e| format!("cannot create {}: {e}", options.archive_dir.display()))?;
    // Zero-padded so a directory listing is in history order.
    let archive = options
        .archive_dir
        .join(format!("events-{first:010}-{last:010}.db"));
    let done = engine
        .compact(now()?, &archive)
        .map_err(|e| e.to_string())?;
    Ok(vec![
        format!(
            "compacted: {} events moved to {}",
            done.events,
            archive.display()
        ),
        format!("snapshot at {}", done.head),
        format!("log at {}", options.db.display()),
        format!("public key {}", public.to_hex()),
        format!(
            "Keep that archive: `hyalos log verify --archive {}` checks it, and `hyalos log prune` removes it when you choose to.",
            archive.display()
        ),
    ])
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
             to add to this log the engine needs the original engine.key: put it in the config directory with \
             mode 0600, then run `hyalos init` (docs/operations/keys-and-recovery.md)."
        );
    }
}

fn parse(command: Command, args: &[String], env: Env<'_>) -> Result<Options, String> {
    let mut db = None;
    let mut key_hex = None;
    let mut key_file = None;
    let mut expect = None;
    let mut archives: Vec<PathBuf> = Vec::new();
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
            "--archive" if command == Command::Verify => archives.push(PathBuf::from(value()?)),
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
        archives,
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
    fn verify_takes_any_number_of_archives_and_nothing_else_does() {
        let o = parse(
            Command::Verify,
            &args(&[
                "--db",
                "a.db",
                "--public-key",
                KEY,
                "--archive",
                "x.db",
                "--archive",
                "y.db",
            ]),
            &nowhere,
        )
        .unwrap();
        assert_eq!(
            o.archives,
            vec![PathBuf::from("x.db"), PathBuf::from("y.db")]
        );
        for command in [Command::Backup, Command::Restore] {
            let err = parse(
                command,
                &args(&[
                    "f.db",
                    "--db",
                    "a.db",
                    "--public-key",
                    KEY,
                    "--archive",
                    "x.db",
                    "--yes",
                ]),
                &nowhere,
            )
            .unwrap_err();
            assert!(err.contains("unknown argument --archive"), "{err}");
        }
    }

    #[test]
    fn prune_needs_one_archive_an_explicit_yes_and_the_engines_key() {
        let with = |extra: &[&str]| parse_prune(&args(extra), &at("/h"));
        let o = with(&["old.db", "--yes"]).unwrap();
        assert_eq!(o.archive, PathBuf::from("old.db"));
        assert_eq!(o.db, PathBuf::from("/h/data/events.db"));
        assert_eq!(o.key_file, PathBuf::from("/h/config/engine.key"));
        let o = with(&["old.db", "--yes", "--db", "x.db", "--key-file", "k"]).unwrap();
        assert_eq!(o.db, PathBuf::from("x.db"));
        assert_eq!(o.key_file, PathBuf::from("k"));
        let err = with(&["old.db"]).unwrap_err();
        assert!(err.contains("--yes") && err.contains("for good"), "{err}");
        assert!(with(&["--yes"]).unwrap_err().contains("needs the archive"));
        assert!(with(&["a.db", "b.db", "--yes"]).is_err());
        // It signs, so it takes no public key; and --yes belongs to prune alone.
        assert!(with(&["a.db", "--yes", "--public-key", KEY]).is_err());
        assert!(
            parse(
                Command::Verify,
                &args(&["--yes", "--db", "a.db", "--public-key", KEY]),
                &nowhere
            )
            .is_err()
        );
    }

    #[test]
    fn compact_defaults_to_the_data_and_config_directories() {
        let o = parse_compact(&args(&[]), &at("/h")).unwrap();
        assert_eq!(o.db, PathBuf::from("/h/data/events.db"));
        assert_eq!(o.key_file, PathBuf::from("/h/config/engine.key"));
        assert_eq!(o.archive_dir, PathBuf::from("/h/data/archive"));
    }

    #[test]
    fn compact_flags_override_and_archives_follow_the_log() {
        let o = parse_compact(
            &args(&[
                "--db",
                "/x/log.db",
                "--key-file",
                "/k/key",
                "--archive-dir",
                "/arch",
            ]),
            &nowhere,
        )
        .unwrap();
        assert_eq!(
            (o.db, o.key_file, o.archive_dir),
            (
                PathBuf::from("/x/log.db"),
                PathBuf::from("/k/key"),
                PathBuf::from("/arch")
            )
        );
        let beside = parse_compact(
            &args(&["--db", "/x/log.db", "--key-file", "/k/key"]),
            &nowhere,
        )
        .unwrap();
        assert_eq!(beside.archive_dir, PathBuf::from("/x/archive"));
        assert!(
            parse_compact(
                &args(&["--db", "l.db", "--key-file", "k", "extra"]),
                &nowhere
            )
            .is_err()
        );
        assert!(parse_compact(&args(&["--bogus"]), &nowhere).is_err());
        assert!(parse_compact(&args(&["--db"]), &nowhere).is_err());
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
