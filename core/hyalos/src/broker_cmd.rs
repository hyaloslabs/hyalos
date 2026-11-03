//! `hyalos broker <socket>`: the broker process, started by the engine (ADR-0002, ADR-0035).
//!
//! It is not a command an operator runs. It connects to the engine's socket and goes on only if the
//! process there is its parent and the same user; it then serves the engine over that one connection
//! until the engine says stop or closes it.

use std::path::Path;
use std::process::ExitCode;

use hyalos_broker::{Exit, serve};

use crate::channel::connect_to_engine;
use crate::clock::SystemClock;

pub fn run(args: &[String]) -> ExitCode {
    let [socket] = args else {
        eprintln!("hyalos broker: started by the engine with the path of its socket, not by hand");
        return ExitCode::from(2);
    };
    let stream = match connect_to_engine(Path::new(socket)) {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("hyalos broker: {e}");
            return ExitCode::from(1);
        }
    };
    let (mut input, mut output) = (&stream, &stream);
    match serve(&mut input, &mut output, &SystemClock) {
        Ok(Exit::Shutdown | Exit::EngineClosed) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hyalos broker: {e}");
            ExitCode::from(1)
        }
    }
}
