//! `hyalos log verify`: check an event log from the outside (ADR-0018).
//!
//! The public key comes from the operator, never from the log: from `--public-key`,
//! `--public-key-file`, or by default the `engine.pub` file the engine wrote when it created its key.
//! The database is opened read-only.

use std::path::PathBuf;
use std::process::ExitCode;

use hyalos_log::{Head, PublicKey, verify};

use crate::paths::{self, Paths};

const USAGE: &str = "usage: hyalos log verify [--db <path>] [--public-key <hex> | --public-key-file <path>] [--expect-head <seq>:<hash>]
  --db defaults to events.db in the data directory; the key defaults to engine.pub in the config directory
  (HYALOS_DATA_DIR / HYALOS_CONFIG_DIR override both; see ADR-0018)";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    db: PathBuf,
    key: PublicKey,
    /// Where the key came from, shown with the result so the operator sees what was trusted.
    key_source: String,
    expect_head: Option<Head>,
}

pub fn run(args: &[String]) -> ExitCode {
    if args.first().map(String::as_str) != Some("verify") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let resolve = || paths::resolve(&|name| std::env::var(name).ok());
    let options = match parse(args.get(1..).unwrap_or_default(), &resolve) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match verify(&options.db, &options.key, options.expect_head) {
        Ok(v) => {
            match v.head {
                Some(head) => println!("ok: {} events, head {head}", v.events),
                None => println!("ok: 0 events"),
            }
            println!(
                "public key {} ({})",
                options.key.to_hex(),
                options.key_source
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse(args: &[String], resolve: &dyn Fn() -> Result<Paths, String>) -> Result<Options, String> {
    let mut db = None;
    let mut key_hex = None;
    let mut key_file = None;
    let mut expect = None;
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
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if key_hex.is_some() && key_file.is_some() {
        return Err("give --public-key or --public-key-file, not both".into());
    }

    let db = match db {
        Some(path) => PathBuf::from(path),
        None => resolve()?.log(),
    };
    let (key_text, key_source) = match (key_hex, key_file) {
        (Some(hex), _) => (hex, "from --public-key".to_string()),
        (None, Some(path)) => (read_key_file(&path)?, format!("from {path}")),
        (None, None) => {
            let path = resolve()?.engine_public_key();
            let shown = path.display().to_string();
            if !path.exists() {
                return Err(format!(
                    "no public key was given and there is none at {shown}; pass --public-key or --public-key-file"
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

    fn nowhere() -> Result<Paths, String> {
        Err("cannot find a home directory; set HYALOS_DATA_DIR to an absolute path".into())
    }

    fn at(dir: &str) -> impl Fn() -> Result<Paths, String> {
        let dir = PathBuf::from(dir);
        move || {
            Ok(Paths {
                data: dir.join("data"),
                config: dir.join("config"),
            })
        }
    }

    #[test]
    fn explicit_flags_need_no_directories() {
        let o = parse(&args(&["--db", "a.db", "--public-key", KEY]), &nowhere).unwrap();
        assert_eq!(o.db, PathBuf::from("a.db"));
        assert_eq!(o.key, PublicKey::from_bytes([1; 32]));
        assert_eq!(o.key_source, "from --public-key");
        assert_eq!(o.expect_head, None);
    }

    #[test]
    fn the_database_defaults_to_the_data_directory() {
        let o = parse(&args(&["--public-key", KEY]), &at("/h")).unwrap();
        assert_eq!(o.db, PathBuf::from("/h/data/events.db"));
    }

    #[test]
    fn a_missing_default_key_file_says_what_to_do() {
        let err = parse(&args(&["--db", "a.db"]), &at("/definitely/not/here")).unwrap_err();
        assert!(
            err.contains("none at /definitely/not/here/config/engine.pub"),
            "{err}"
        );
        assert!(err.contains("--public-key or --public-key-file"), "{err}");
    }

    #[test]
    fn missing_directories_are_reported_only_when_a_default_is_needed() {
        let err = parse(&args(&["--public-key", KEY]), &nowhere).unwrap_err();
        assert!(err.contains("set HYALOS_DATA_DIR"), "{err}");
        assert!(parse(&args(&["--db", "a.db", "--public-key", KEY]), &nowhere).is_ok());
    }

    #[test]
    fn a_head_is_parsed() {
        let head = format!("7:{}", "ab".repeat(32));
        let o = parse(
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
            let err = parse(&args(list), &nowhere).unwrap_err();
            assert!(err.contains(expected), "{list:?}: {err}");
        }
    }
}
