//! `hyalos broker <socket> [<provider-keys>] [--sockets <dir>]... [--read <dir>]...`: the broker
//! process, started by the engine (ADR-0002, ADR-0035, ADR-0038).
//!
//! It is not a command an operator runs. It reads the provider key file, if it was given one, before
//! anything else; then it connects to the engine's socket and goes on only if the process there is
//! its parent and the same user; it then serves the engine over that one connection, and each
//! admitted job's sandbox on that job's socket, until the engine says stop or closes the channel.
//! Before it serves, it confines itself to the directories the engine named (ADR-0118).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use hyalos_broker::egress::Egress;
use hyalos_broker::git::Git;
use hyalos_broker::model::ThreadSleep;
use hyalos_broker::sandbox::{Models, Providers, Sandboxes};
use hyalos_broker::{Exit, Plugins, serve_with_plugins};
use hyalos_plugin::Runtime;
use hyalos_plugin::skill::Store;
use hyalos_types::ReasonCode;
use hyalos_types::wire::PluginCall;
use hyalos_types::workflow::TypedValue;

use crate::channel::connect_to_engine;
use crate::clock::SystemClock;
use crate::confine::{self, Confined, Grants};
use crate::keys::read_private;

/// A key file is a few lines; anything larger is not one.
const KEY_FILE_LIMIT: u64 = 16 * 1024;

/// The broker's arguments: its socket, the provider key file if any, and what it may reach once
/// confined. `None` for anything else.
fn parse(args: &[String]) -> Option<(&str, Option<&Path>, Grants)> {
    let mut grants = Grants::default();
    let mut positional = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--sockets" => grants.sockets.push(PathBuf::from(rest.next()?)),
            "--read" => grants.reads.push(PathBuf::from(rest.next()?)),
            _ => positional.push(arg.as_str()),
        }
    }
    match positional[..] {
        [socket] => Some((socket, None, grants)),
        [socket, keys] => Some((socket, Some(Path::new(keys)), grants)),
        _ => None,
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let Some((socket, keys, grants)) = parse(args) else {
        eprintln!("hyalos broker: started by the engine with the path of its socket, not by hand");
        return ExitCode::from(2);
    };
    let providers = match keys.map(providers).transpose() {
        Ok(providers) => providers.unwrap_or_default(),
        Err(e) => {
            eprintln!("hyalos broker: {e}");
            return ExitCode::from(1);
        }
    };
    let sandboxes = Sandboxes {
        models: Arc::new(Models::from_providers(&providers, Arc::new(ThreadSleep))),
        // The providers' own hosts are never egress destinations (ADR-0051).
        egress: Egress::public().refusing_bases(providers.bases()),
        git: Git::github(),
        forge: hyalos_broker::forge::Forge::github(),
        clock: Arc::new(SystemClock),
    };
    drop(providers);
    let stream = match connect_to_engine(Path::new(socket)) {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("hyalos broker: {e}");
            return ExitCode::from(1);
        }
    };
    let (mut input, mut output) = (&stream, &stream);
    // Plugins run here, in the broker (ADR-0087). Without a runtime, a deterministic step is refused.
    let steps = Runtime::new()
        .ok()
        .map(|runtime| Arc::new(Steps(runtime)) as Arc<dyn Plugins>);
    // Everything read at startup is read: from here on the broker reaches only what it was granted,
    // never the engine's keys (ADR-0118). A broker that cannot confine itself does not serve
    // (ADR-0121).
    match confine::apply(&grants) {
        Ok(Confined::Fully | Confined::Partly) => {}
        Ok(Confined::Not) => {
            eprintln!("hyalos broker: {}", confine::NO_LANDLOCK);
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("hyalos broker: {e}");
            return ExitCode::from(1);
        }
    }
    match serve_with_plugins(
        &mut input,
        &mut output,
        &SystemClock,
        Some(sandboxes),
        steps,
    ) {
        Ok(Exit::Shutdown | Exit::EngineClosed) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hyalos broker: {e}");
            ExitCode::from(1)
        }
    }
}

/// The providers in the key file. A file that is missing means no provider yet, and model calls are
/// refused; one that is not the operator's alone, or does not parse, stops the broker.
pub(crate) fn providers(path: &Path) -> Result<Providers, String> {
    if !path.exists() {
        return Ok(Providers::default());
    }
    let raw = read_private(path, KEY_FILE_LIMIT).map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&raw).map_err(|_| format!("{} is not text", path.display()))?;
    Providers::parse(text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs a deterministic step's plugin (ADR-0102): read from the store and checked against the digest
/// the engine named, then run with no capability.
struct Steps(Runtime);

impl std::fmt::Debug for Steps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Steps")
    }
}

impl Plugins for Steps {
    fn run(&self, call: &PluginCall) -> Result<Vec<TypedValue>, ReasonCode> {
        let skill = Store::new(&call.store)
            .load(&call.digest, &call.name)
            .map_err(|e| e.code())?;
        hyalos_plugin::run_step(&self.0, &skill, &call.input, &call.output)
    }
}
