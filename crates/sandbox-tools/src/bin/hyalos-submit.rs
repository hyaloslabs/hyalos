//! `hyalos-submit [<json>]`: posts a job's result (the argument, or standard input) to the broker
//! through the forwarder, prints the broker's answer, and succeeds only if the broker accepted it
//! (ADR-0016). `HYALOS_FORWARDER` overrides the forwarder's address.

use std::io::Read;
use std::process::ExitCode;

use hyalos_sandbox_tools::{FORWARDER, MAX_SUBMISSION, submit};

fn main() -> ExitCode {
    let body = match std::env::args().nth(1) {
        Some(arg) => arg.into_bytes(),
        None => {
            let mut body = Vec::new();
            let read = std::io::stdin()
                .take(MAX_SUBMISSION as u64 + 1)
                .read_to_end(&mut body);
            if let Err(e) = read {
                eprintln!("hyalos-submit: cannot read the result: {e}");
                return ExitCode::from(2);
            }
            body
        }
    };
    let forwarder = std::env::var("HYALOS_FORWARDER").unwrap_or_else(|_| FORWARDER.into());
    match submit(&forwarder, &body) {
        Ok((status, answer)) => {
            println!("{}", String::from_utf8_lossy(&answer));
            if (200..300).contains(&status) {
                ExitCode::SUCCESS
            } else {
                eprintln!("hyalos-submit: the broker refused the result ({status})");
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("hyalos-submit: {e}");
            ExitCode::from(1)
        }
    }
}
