//! A test-only notary: the real program, signing the ticket bytes `HYALOS_FORGE_TICKET` names. It
//! exists because a real notary never signs a bad ticket, so the audit's checks on one could not
//! otherwise be reached. The hook is defined only here, so `notary` cannot compile with the
//! `forge-ticket` feature on.
//!
//! The setting is `missing`, `doubled`, `short`, or a comma-separated list of `field=value` for
//! `authorisation`, `task`, `budget`, `round`, `prior` and `ticket`. A `later:` prefix leaves a
//! run's first session alone.
//!
//! `HYALOS_FORGE_SESSION_LATER` adds that many seconds to the session time the signing check judges,
//! so a pass can go stale between a session's open and its signing.
//!
//! `HYALOS_FORGE_CANARY` is what it signs in place of the pass: `missing`, `doubled`, `malformed` (7
//! bytes), `at=<unix seconds>`, or `before=<seconds>` before the signed session time.

use attest::Ticket;

#[path = "notary.rs"]
mod notary;

/// The ticket extensions to sign in place of the real one.
pub(crate) fn forged_ticket(real: &Ticket, sequence: u64) -> Vec<Vec<u8>> {
    let Ok(said) = std::env::var("HYALOS_FORGE_TICKET") else { return vec![real.bytes()] };
    let (first, said) = said.strip_prefix("later:").map_or((0, said.as_str()), |rest| (1, rest));
    if sequence < first {
        return vec![real.bytes()];
    }
    match said {
        "missing" => Vec::new(),
        "doubled" => vec![real.bytes(), real.bytes()],
        "short" => {
            let mut bytes = real.bytes();
            bytes.pop();
            vec![bytes]
        }
        edits => {
            let mut forged = real.clone();
            for edit in edits.split(',') {
                let (field, value) = edit.split_once('=').expect("a forged field");
                let number = || value.parse::<u32>().expect("a forged number");
                match field {
                    "authorisation" => forged.authorisation = value.to_string(),
                    "task" => forged.task = value.to_string(),
                    "budget" => forged.budget = number(),
                    "round" => forged.round = number(),
                    "prior" => forged.prior = number(),
                    "ticket" => forged.ticket = number(),
                    _ => panic!("no such field"),
                }
            }
            vec![forged.bytes()]
        }
    }
}

/// The canary extensions to sign in place of the real one.
pub(crate) fn forged_canary(real: u64, session: u64) -> Vec<Vec<u8>> {
    let pass = real.to_be_bytes().to_vec();
    let Ok(said) = std::env::var("HYALOS_FORGE_CANARY") else { return vec![pass] };
    match said.as_str() {
        "missing" => Vec::new(),
        "doubled" => vec![pass.clone(), pass],
        "malformed" => vec![pass[..7].to_vec()],
        other => {
            let at = match other.split_once('=').expect("a forged canary") {
                ("at", value) => value.parse::<u64>().expect("a forged time"),
                ("before", value) => session - value.parse::<u64>().expect("a forged age"),
                _ => panic!("no such canary"),
            };
            vec![at.to_be_bytes().to_vec()]
        }
    }
}

/// The session time the signing check judges.
pub(crate) fn forged_session_time(real: u64) -> u64 {
    std::env::var("HYALOS_FORGE_SESSION_LATER").map_or(real, |later| real + later.parse::<u64>().expect("a number of seconds"))
}

fn main() -> std::process::ExitCode {
    notary::main()
}
