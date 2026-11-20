//! `hyalos policy check|approve|show`: the operator's Cedar policy (ADR-0008, ADR-0034, ADR-0050).
//!
//! The shipped baseline forbids, and Cedar allows nothing that no policy permits, so until the
//! operator approves a policy no manifest can be signed and no job runs. `approve` records the
//! document as approved by a human, the operator at this terminal, which is the only decider the
//! state accepts for a policy change; the approval is signed with the operator's key (ADR-0070).

use std::process::ExitCode;

use hyalos_engine::{Decider, Engine};
use hyalos_types::{MAX_POLICY_DOCUMENT, ReasonCode};

use crate::clock::SystemClock;
use crate::paths::{self, Env};
use crate::{keys, operator, passphrase};

/// A starter policy for a first job (S5.5): the fix-issue workflow's actions, data only to the model
/// and the forge. The operator reads it and approves it; nothing puts it in force by itself.
pub const STARTER: &str = include_str!("../policies/starter.cedar");

const USAGE: &str = "usage: hyalos policy check <file>
       hyalos policy approve <file> [--reason <code>] [--passphrase-stdin]
       hyalos policy show
       hyalos policy starter
  check says whether <file> is Cedar that the shipped schema accepts, and writes nothing
  approve records <file> as the operator's policy, approved by you, from this event on; it is checked
  first and nothing is written if it fails. --reason is a short code (default policy.approved). It is
  signed with your approval key: the passphrase is asked for at the terminal, or read from stdin
  show prints the policy in force, or says there is none
  starter prints a starter policy for a first job, to read before you approve it
  The log is events.db in the data directory and the key engine.key in the config directory
  (HYALOS_DATA_DIR / HYALOS_CONFIG_DIR override both)";

pub fn run(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    crate::operator_cmd::finish(run_with(args, &env))
}

/// The command's output, or its exit code and message.
pub fn run_with(args: &[String], env: Env<'_>) -> Result<String, (u8, String)> {
    let (args, source) = passphrase::source(args);
    let usage = |why: &str| (2, format!("{why}\n{USAGE}"));
    let failed = |e: String| (1, e);
    match args.as_slice() {
        [starter] if starter == "starter" => Ok(STARTER.to_string()),
        [check, file] if check == "check" => {
            let document = read(file).map_err(failed)?;
            Engine::check_policy(&document).map_err(|e| failed(format!("{file}: {e}")))?;
            Ok(format!("{file}: valid against the shipped schema\n"))
        }
        [approve, file, rest @ ..] if approve == "approve" => {
            let reason = match rest {
                [] => "policy.approved".to_string(),
                [flag, code] if flag == "--reason" => code.clone(),
                _ => return Err(usage("approve takes only --reason <code>")),
            };
            let reason = ReasonCode::new(&reason)
                .ok_or_else(|| usage("a reason is a short code, such as policy.approved"))?;
            let document = read(file).map_err(failed)?;
            // Checked before the passphrase is asked for, so a bad file costs nothing.
            Engine::check_policy(&document).map_err(|e| failed(format!("{file}: {e}")))?;
            let mut engine = operator::engine(env, source).map_err(failed)?;
            let at = hyalos_types::Clock::now(&SystemClock);
            let head = engine
                .approve_policy(at, &document, Decider::Human, reason)
                .map_err(|e| failed(format!("{file}: {e}")))?;
            Ok(format!(
                "approved {file} as the policy in force from event {}\n",
                head.seq
            ))
        }
        [show] if show == "show" => {
            if !paths::log_file(env).map_err(failed)?.exists() {
                return Ok("there is no log yet, so no policy is approved\n".into());
            }
            let engine = open(env).map_err(failed)?;
            Ok(match engine.state().policy() {
                Some(policy) => format!(
                    "# approved at event {}\n{}\n",
                    policy.seq,
                    policy.document.trim_end()
                ),
                None => {
                    "no policy is approved: the baseline alone is in force, and it allows no job\n"
                        .to_string()
                }
            })
        }
        _ => Err(usage("unknown policy command")),
    }
}

fn read(file: &str) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("cannot read {file}: {e}"))?;
    if bytes.len() > MAX_POLICY_DOCUMENT {
        return Err(format!(
            "{file} is larger than a policy may be ({MAX_POLICY_DOCUMENT} bytes)"
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{file} is not text"))
}

/// The engine on the operator's log, with the engine's key: for a command that writes an event.
pub(crate) fn open(env: Env<'_>) -> Result<Engine, String> {
    let key_file = paths::config_dir(env)?.join(paths::ENGINE_KEY_FILE);
    let key = keys::load(&key_file).map_err(|e| e.to_string())?;
    // The first event creates the log, as it does for the engine.
    let db = paths::log_file(env)?;
    Engine::open(&db, key).map_err(|e| e.to_string())
}
