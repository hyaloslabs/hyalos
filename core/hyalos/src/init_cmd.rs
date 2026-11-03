//! `hyalos init`: make the engine's signing key ready (ADR-0022).
//!
//! On a new machine it creates the directories, the key and `engine.pub`. After a restore, with
//! `engine.key` put back in place, it rewrites a missing `engine.pub` and checks the key against the
//! log. It is safe to run again. Starting the engine will do the same thing first.

use std::process::ExitCode;

use crate::keys::{self, OsSeed};
use crate::paths;

const USAGE: &str = "usage: hyalos init
  creates the engine's signing key (engine.key) and its public half (engine.pub) in the config directory,
  and the data directory, if there is no key and no log. Run again, it checks what is there.
  HYALOS_DATA_DIR / HYALOS_CONFIG_DIR override the directories; see ADR-0018 and ADR-0022";

pub fn run(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("hyalos: init takes no arguments\n{USAGE}");
        return ExitCode::from(2);
    }
    let env = |name: &str| std::env::var(name).ok();
    let (config, log) = match (paths::config_dir(&env), paths::log_file(&env)) {
        (Ok(config), Ok(log)) => (config, log),
        (Err(message), _) | (_, Err(message)) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match keys::prepare(&config, &log, &mut OsSeed) {
        Ok(ready) if ready.created => {
            println!("created the engine's signing key");
            println!(
                "  key         {}  (mode 0600)",
                config.join(paths::ENGINE_KEY_FILE).display()
            );
            println!(
                "  public key  {}  (mode 0444)",
                config.join(paths::ENGINE_PUBLIC_KEY_FILE).display()
            );
            println!("public key {}", ready.public.to_hex());
            println!();
            println!(
                "Back up engine.key now, somewhere a compromise of this machine cannot reach (a password"
            );
            println!(
                "manager, offline media). Without it this log can be verified but never extended."
            );
            println!(
                "Keep a copy of the public key too. See docs/operations/keys-and-recovery.md."
            );
            ExitCode::SUCCESS
        }
        Ok(ready) => {
            println!("engine key ok: public key {}", ready.public.to_hex());
            if ready.public_rewritten {
                println!(
                    "wrote {} from the key",
                    config.join(paths::ENGINE_PUBLIC_KEY_FILE).display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hyalos: {e}");
            ExitCode::FAILURE
        }
    }
}
