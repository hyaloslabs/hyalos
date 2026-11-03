//! The `hyalos` binary. The engine and broker run from this one executable (ADR-0002).

mod init_cmd;
mod keys;
mod log_cmd;
mod paths;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--version") => {
            println!("hyalos {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("init") => init_cmd::run(args.get(1..).unwrap_or_default()),
        Some("log") => log_cmd::run(args.get(1..).unwrap_or_default()),
        Some(other) => {
            eprintln!(
                "hyalos: unknown command {other}\nusage: hyalos [--version | init | log verify|backup|restore ...]"
            );
            ExitCode::from(2)
        }
    }
}
