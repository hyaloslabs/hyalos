//! `hyalos-pr <owner/repo> <head> <base> <title> [<body-file>]`: asks the broker, through the
//! forwarder, to open a pull request from branch `head` into `base` (ADR-0048, ADR-0050). The body is
//! the file named, or nothing. Prints the broker's answer (the pull request's number and page) and
//! succeeds only if the pull request was opened. `HYALOS_FORWARDER` overrides the forwarder's address.
//!
//! The broker decides everything that matters: the repository, head and base must be the manifest's,
//! and the title and body pass its content gate. This only spares a harness writing the request.

use std::io::Read;
use std::process::ExitCode;

use hyalos_sandbox_tools::{FORWARDER, MAX_SUBMISSION, json_string, post};

const USAGE: &str = "usage: hyalos-pr <owner/repo> <head> <base> <title> [<body-file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (repo, head, base, title, body_file) = match args.as_slice() {
        [repo, head, base, title] => (repo, head, base, title, None),
        [repo, head, base, title, file] => (repo, head, base, title, Some(file)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if repo.split('/').count() != 2 || repo.contains(['?', '#', ' ']) {
        eprintln!("hyalos-pr: the repository is owner/name\n{USAGE}");
        return ExitCode::from(2);
    }
    let body = match body_file {
        None => String::new(),
        Some(path) => {
            let mut text = String::new();
            let read = std::fs::File::open(path)
                .and_then(|f| f.take(MAX_SUBMISSION as u64).read_to_string(&mut text));
            if let Err(e) = read {
                eprintln!("hyalos-pr: cannot read {path}: {e}");
                return ExitCode::from(2);
            }
            text
        }
    };
    let request = format!(
        "{{\"title\":{},\"body\":{},\"head\":{},\"base\":{}}}",
        json_string(title),
        json_string(&body),
        json_string(head),
        json_string(base)
    );
    let forwarder = std::env::var("HYALOS_FORWARDER").unwrap_or_else(|_| FORWARDER.into());
    match post(
        &forwarder,
        &format!("/forge/repos/{repo}/pulls"),
        request.as_bytes(),
    ) {
        Ok((status, answer)) => {
            println!("{}", String::from_utf8_lossy(&answer).trim_end());
            if status == 201 {
                ExitCode::SUCCESS
            } else {
                eprintln!("hyalos-pr: the broker did not open the pull request ({status})");
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("hyalos-pr: {e}");
            ExitCode::from(1)
        }
    }
}
