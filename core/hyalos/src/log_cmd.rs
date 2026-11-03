//! `hyalos log verify`: check an event log from the outside (ADR-0018).
//!
//! The public key comes from the operator, never from the log, and the database is opened read-only.

use std::path::PathBuf;
use std::process::ExitCode;

use hyalos_log::{Head, PublicKey, verify};

const USAGE: &str = "usage: hyalos log verify --db <path> (--public-key <hex> | --public-key-file <path>) [--expect-head <seq>:<hash>]";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    db: PathBuf,
    key: PublicKey,
    expect_head: Option<Head>,
}

pub fn run(args: &[String]) -> ExitCode {
    if args.first().map(String::as_str) != Some("verify") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let options = match parse(args.get(1..).unwrap_or_default()) {
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
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse(args: &[String]) -> Result<Options, String> {
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
    let db = PathBuf::from(db.ok_or("--db is required")?);
    let key_text = match (key_hex, key_file) {
        (Some(hex), None) => hex,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read the public key file {path}: {e}"))?,
        (None, None) => {
            return Err("a public key is required: --public-key or --public-key-file".into());
        }
        (Some(_), Some(_)) => return Err("give --public-key or --public-key-file, not both".into()),
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
        expect_head,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    const KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    #[test]
    fn a_minimal_command_parses() {
        let o = parse(&args(&["--db", "a.db", "--public-key", KEY])).unwrap();
        assert_eq!(o.db, PathBuf::from("a.db"));
        assert_eq!(o.key, PublicKey::from_bytes([1; 32]));
        assert_eq!(o.expect_head, None);
    }

    #[test]
    fn a_head_is_parsed() {
        let head = format!("7:{}", "ab".repeat(32));
        let o = parse(&args(&[
            "--db",
            "a.db",
            "--public-key",
            KEY,
            "--expect-head",
            &head,
        ]))
        .unwrap();
        assert_eq!(o.expect_head.map(|h| h.seq), Some(7));
    }

    #[test]
    fn mistakes_are_refused_with_a_reason() {
        let cases: [(&[&str], &str); 7] = [
            (&["--public-key", KEY], "--db is required"),
            (&["--db", "a.db"], "a public key is required"),
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
            let err = parse(&args(list)).unwrap_err();
            assert!(err.contains(expected), "{list:?}: {err}");
        }
    }
}
