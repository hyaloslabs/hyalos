//! `policy-diff <old-baseline.cedar>`: says whether the shipped baseline allows more or less than the
//! given earlier version, as Markdown on stdout. CI runs it on pull requests that touch the baseline
//! and adds the output to the job summary, so a change that allows more is visible in review. It exits
//! 0 whatever it finds (a reviewer decides) and 1 if it could not compare.

use std::process::ExitCode;

use hyalos_policy::BASELINE;
use hyalos_policy_proof::{Prover, markdown};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: policy-diff <old-baseline.cedar>");
        return ExitCode::from(2);
    };
    let result = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {path}: {e}"))
        .and_then(|old| Prover::new()?.compare(&old, BASELINE));
    match result {
        Ok(comparison) => {
            print!("{}", markdown(&comparison));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("policy-diff: {message}");
            ExitCode::FAILURE
        }
    }
}
