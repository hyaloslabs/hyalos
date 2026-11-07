//! `hyalos operator init|show`, `hyalos approve <hold>` and `hyalos cancel <task>`: the operator's
//! own channel for decisions (ADR-0010, ADR-0012, ADR-0070).
//!
//! A decision is recorded only after the operator's passphrase opens `operator.key`, and it carries
//! the operator's signature as well as the engine's. The MCP server offers no tool that decides and
//! never opens the operator's key, and the driving agent's credential signs nothing.

use std::process::ExitCode;

use hyalos_engine::Decider;
use hyalos_types::{HoldId, ReasonCode, TaskId};

use crate::clock::SystemClock;
use crate::keys::OsSeed;
use crate::operator;
use crate::passphrase::{self, Source};
use crate::paths::{self, Env, OPERATOR_PUBLIC_KEY_FILE};

const OPERATOR_USAGE: &str = "usage: hyalos operator init [--passphrase-stdin]
       hyalos operator show
  init creates your approval key, operator.key, sealed under a passphrase you choose (at least 12
  characters), and its public half operator.pub, in the config directory. Every human decision is
  signed with it. It is never replaced; keep the passphrase from the driving agent
  show prints the public key in operator.pub
  The passphrase is asked for at the terminal; --passphrase-stdin reads one line from stdin instead";

const APPROVE_USAGE: &str = "usage: hyalos approve <hold> [--reason <code>] [--passphrase-stdin]
  releases hold <hold> as your decision, signed with your approval key after your passphrase.
  --reason is a short code (default approved.operator)";

const CANCEL_USAGE: &str = "usage: hyalos cancel <task> [--reason <code>] [--passphrase-stdin]
  ends held task <task> as your decision, signed with your approval key after your passphrase.
  --reason is a short code (default cancelled.operator)";

fn finish(result: Result<String, (u8, String)>) -> ExitCode {
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err((code, message)) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(code)
        }
    }
}

fn env() -> impl Fn(&str) -> Option<String> {
    |name: &str| std::env::var(name).ok()
}

pub fn run(args: &[String]) -> ExitCode {
    finish(operator_with(args, &env()))
}

pub fn approve(args: &[String]) -> ExitCode {
    finish(approve_with(args, &env()))
}

pub fn cancel(args: &[String]) -> ExitCode {
    finish(cancel_with(args, &env()))
}

pub fn operator_with(args: &[String], env: Env<'_>) -> Result<String, (u8, String)> {
    let (args, source) = passphrase::source(args);
    let usage = |why: &str| (2, format!("{why}\n{OPERATOR_USAGE}"));
    let failed = |e: String| (1, e);
    match args.as_slice() {
        [init] if init == "init" => {
            let config_dir = paths::config_dir(env).map_err(failed)?;
            let words = passphrase::read(source, "new operator passphrase: ").map_err(failed)?;
            if source == Source::Terminal {
                let again = passphrase::read(source, "again: ").map_err(failed)?;
                if again != words {
                    return Err(failed(
                        "the two passphrases differ; nothing was written".into(),
                    ));
                }
            }
            let public = operator::create(&config_dir, &words, &mut OsSeed).map_err(failed)?;
            Ok(format!(
                "created your approval key; its public half is in {}:\n{public}\nKeep the passphrase from the driving agent: whoever has it can sign your decisions.\n",
                config_dir.join(OPERATOR_PUBLIC_KEY_FILE).display()
            ))
        }
        [show] if show == "show" => {
            let path = paths::config_dir(env)
                .map_err(failed)?
                .join(OPERATOR_PUBLIC_KEY_FILE);
            Ok(format!(
                "{}\n",
                operator::read_public(&path).map_err(failed)?
            ))
        }
        _ => Err(usage("unknown operator command")),
    }
}

/// `<id> [--reason <code>]`, with the flag for the passphrase already taken out.
fn id_and_reason(args: &[String], default: &str) -> Option<(u64, ReasonCode)> {
    let (id, rest) = args.split_first()?;
    let reason = match rest {
        [] => default,
        [flag, code] if flag == "--reason" => code,
        _ => return None,
    };
    Some((id.parse().ok()?, ReasonCode::new(reason)?))
}

pub fn approve_with(args: &[String], env: Env<'_>) -> Result<String, (u8, String)> {
    let (args, source) = passphrase::source(args);
    let (id, reason) = id_and_reason(&args, "approved.operator").ok_or_else(|| {
        (
            2,
            format!("a hold's id is a number and a reason a short code\n{APPROVE_USAGE}"),
        )
    })?;
    let mut engine = operator::engine(env, source).map_err(|e| (1, e))?;
    let at = hyalos_types::Clock::now(&SystemClock);
    let head = engine
        .decide_hold(at, HoldId::from_seq(id), Decider::Human, reason)
        .map_err(|e| (1, format!("hold {id}: {e}")))?;
    Ok(format!(
        "released hold {id} at event {}, signed by you\n",
        head.seq
    ))
}

pub fn cancel_with(args: &[String], env: Env<'_>) -> Result<String, (u8, String)> {
    let (args, source) = passphrase::source(args);
    let (id, reason) = id_and_reason(&args, "cancelled.operator").ok_or_else(|| {
        (
            2,
            format!("a task's id is a number and a reason a short code\n{CANCEL_USAGE}"),
        )
    })?;
    let mut engine = operator::engine(env, source).map_err(|e| (1, e))?;
    let at = hyalos_types::Clock::now(&SystemClock);
    let head = engine
        .cancel_task(at, TaskId::from_seq(id), Decider::Human, reason)
        .map_err(|e| (1, format!("task {id}: {e}")))?;
    Ok(format!(
        "cancelled task {id} at event {}, signed by you\n",
        head.seq
    ))
}
