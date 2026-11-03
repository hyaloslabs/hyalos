//! The `hyalos` binary. The engine and broker run from this one executable (ADR-0002).

use std::process::ExitCode;

use hyalos::{broker_cmd, hardening, init_cmd, log_cmd};

fn main() -> ExitCode {
    // Before anything reads a key: a crash must not be able to write it to disk (ADR-0030).
    if let Err(e) = hardening::no_core_dumps() {
        eprintln!("hyalos: {e}");
        return ExitCode::from(1);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--version") => {
            println!("hyalos {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("init") => init_cmd::run(args.get(1..).unwrap_or_default()),
        Some("log") => log_cmd::run(args.get(1..).unwrap_or_default()),
        // Started by the engine, never by hand; it refuses any other parent (ADR-0035).
        Some("broker") => broker_cmd::run(args.get(1..).unwrap_or_default()),
        Some(other) => {
            eprintln!(
                "hyalos: unknown command {other}\nusage: hyalos [--version | init | log verify|audit|backup|restore|compact|prune ...]"
            );
            ExitCode::from(2)
        }
    }
}
