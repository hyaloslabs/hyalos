//! `hyalos-forward <program> [<args>...]`: relays `127.0.0.1:8089` to the broker's socket at
//! `/run/broker.sock`, makes `$HOME` and `$CODEX_HOME`, links the job's skills into both if it has
//! any, then runs the program and exits as it did.
//! `HYALOS_FORWARDER` and `HYALOS_BROKER_SOCKET` override the two addresses (for tests).

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hyalos_sandbox_tools::{
    BROKER_SOCKET, CODEX_SEED, FORWARDER, SKILLS, forward, link_skills, prepare, run,
};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--") {
        args.remove(0);
    }
    let Some(program) = args.first().cloned() else {
        eprintln!("usage: hyalos-forward <program> [<args>...]");
        return ExitCode::from(2);
    };
    let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
    let path = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if let Err(e) = prepare(
        path("HOME").as_deref(),
        path("CODEX_HOME").as_deref(),
        Path::new(CODEX_SEED),
    ) {
        eprintln!("hyalos-forward: cannot make the harness's directories: {e}");
        return ExitCode::from(1);
    }
    if let Err(e) = link_skills(
        path("HOME").as_deref(),
        path("CODEX_HOME").as_deref(),
        Path::new(SKILLS),
    ) {
        eprintln!("hyalos-forward: cannot show the harness its skills: {e}");
        return ExitCode::from(1);
    }
    // Listening before the program starts, so its first call finds the port open.
    let listener = match TcpListener::bind(var("HYALOS_FORWARDER", FORWARDER)) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("hyalos-forward: cannot listen: {e}");
            return ExitCode::from(1);
        }
    };
    let socket = PathBuf::from(var("HYALOS_BROKER_SOCKET", BROKER_SOCKET));
    if std::thread::Builder::new()
        .spawn(move || forward(listener, socket))
        .is_err()
    {
        eprintln!("hyalos-forward: cannot start");
        return ExitCode::from(1);
    }
    match run(&program, &args[1..]) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("hyalos-forward: cannot run {program}: {e}");
            ExitCode::from(127)
        }
    }
}
