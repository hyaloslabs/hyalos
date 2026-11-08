//! `hyalos setup`: what still stands between this machine and a first job, step by step, and who
//! takes each step (S5.5, ADR-0076). The set-up-hyalos Agent Skill reads it to know what to do next.
//!
//! It is read-only: it writes nothing but what substrate detection caches (as `hyalos doctor` does),
//! reads a private key file only to check that it is the operator's alone and parses, makes no
//! network call, and prints no secret. Each step is the agent's, when it is
//! setup a shell can do, or the operator's, when it needs a secret (a provider key, the App's private
//! key) or is a decision signed with the operator's key behind their passphrase, which the agent must
//! never see. The operator runs those in a terminal of their own.

use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::doctor_cmd::DEFAULT_IMAGE;
use crate::github::GitHubApp;
use crate::mcp_cmd::EngineLog;
use crate::paths::{self, Env};

const USAGE: &str = "usage: hyalos setup [--next] [--image <image>] [--json]
  lists what this machine still needs before a first job, in order, and who takes each step: the
  agent (setup a shell can do) or you (a secret, or a decision signed with your approval key, which
  you run in a terminal of your own). It writes nothing and prints no secret. --next prints only the
  first step not done, ending with its commands ready to paste; --image names the sandbox image
  (default hyalos-sandbox:latest); --json prints the steps as JSON.
  Exit 0 when every step is done, 1 when one is not, 2 for bad usage";

/// Who takes a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// Setup the driving agent may do from a shell: nothing secret, nothing decided.
    Agent,
    /// The operator, in a terminal of their own: a secret or a signed decision.
    Operator,
}

impl Who {
    fn name(self) -> &'static str {
        match self {
            Who::Agent => "agent",
            Who::Operator => "operator",
        }
    }
}

/// One step towards a first job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: &'static str,
    pub done: bool,
    pub who: Who,
    /// What was found, or why it is not done. Never a secret.
    pub detail: String,
    /// What to run or do when it is not done.
    pub action: &'static str,
    /// The commands that take it, one per line, ready to paste into a shell (`#` lines say what to
    /// change). They read a secret only from the terminal, never from an argument (#258).
    pub commands: Vec<String>,
    /// Where the procedure is written down.
    pub doc: &'static str,
}

/// The steps, in the order they are best taken. `substrate` is the substrate a job would run on, or
/// why none would (from `hyalos_substrate::detect_with`, which `run` asks).
pub fn steps(env: Env<'_>, substrate: Result<String, String>) -> Result<Vec<Step>, String> {
    let config = paths::config_dir(env)?;
    let log = EngineLog {
        log: paths::log_file(env)?,
        public_key: paths::engine_public_key(env)?,
    };
    // `hyalos init` makes the key, not the log; the first event the engine writes creates it (#255).
    // As `keys::prepare` judges it, only a path that is not there at all is no log; a missing log is
    // the empty state.
    let state = match std::fs::symlink_metadata(&log.log) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(format!(
            "cannot read the event log {}: {e}",
            log.log.display()
        ))),
        Ok(_) => Some(log.read()),
    };
    let engine = engine_key(&config, state.as_ref());
    let state = state.unwrap_or_else(|| Ok(hyalos_engine::State::default()));
    let mut steps = Vec::new();
    let mut step = |id, who, action, commands: Vec<String>, doc, found: Result<String, String>| {
        let (done, detail) = match found {
            Ok(detail) => (true, detail),
            Err(detail) => (false, detail),
        };
        steps.push(Step {
            id,
            done,
            who,
            detail,
            action,
            commands,
            doc,
        });
    };
    let keys_path = paths::provider_keys(env)?;
    let dir = quoted(&config)?;
    let keys_file = quoted(&keys_path)?;
    let only_anthropic = crate::broker_cmd::providers(&keys_path)
        .is_ok_and(|p| p.anthropic.is_some() && p.openai.is_none());
    let (app_file, pem_file) = (
        quoted(&config.join("github.app"))?,
        quoted(&config.join("github-app.pem"))?,
    );
    let lines = |lines: &[&str]| lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();

    step(
        "engine_key",
        Who::Agent,
        "hyalos init",
        lines(&["hyalos init"]),
        "docs/operations/keys-and-recovery.md",
        engine,
    );
    step(
        "operator_key",
        Who::Operator,
        "in a terminal of your own: hyalos operator init",
        lines(&["hyalos operator init"]),
        "docs/operations/approvals.md",
        operator_key(&config),
    );
    step(
        "substrate",
        Who::Agent,
        "from the repository root: docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest . (then hyalos doctor)",
        lines(&[
            "# from the repository root; podman in place of docker if that is what you have",
            "docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest .",
            "hyalos doctor",
        ]),
        "docs/operations/substrates.md",
        substrate.map(|name| format!("a job would run on {name}")),
    );
    step(
        "provider_keys",
        Who::Operator,
        "write provider.keys in the config directory with a dedicated key, chmod 600 it, and set a spend cap at the provider",
        vec![
            "# a dedicated key with a spend cap set at the provider: asks which provider and the key (not"
                .to_string(),
            "# shown), and writes provider.keys owner-only. Safe to paste again.".to_string(),
            PROVIDER_KEY_LINE
                .replace("@DIR@", &dir)
                .replace("@FILE@", &keys_file),
        ],
        "docs/operations/provider-keys.md",
        provider_keys(env),
    );
    step(
        "github_app",
        Who::Operator,
        "create the GitHub App, save its private key as github-app.pem (chmod 600) and its id in github.app, and install it on the repository",
        vec![
            "# after creating the App and installing it on the repository: asks for its App ID and the"
                .to_string(),
            "# .pem the browser saved, moves the key into place owner-only and writes github.app"
                .to_string(),
            GITHUB_APP_LINE
                .replace("@DIR@", &dir)
                .replace("@PEM@", &pem_file)
                .replace("@APP@", &app_file),
        ],
        "docs/operations/github-app.md",
        GitHubApp::load(&config)
            .map(|_| "github.app and github-app.pem read".to_string())
            .map_err(|e| e.to_string()),
    );
    step(
        "policy",
        Who::Operator,
        "in a terminal of your own: hyalos policy starter > policy.cedar, read it, then hyalos policy approve policy.cedar",
        lines(&[
            "hyalos policy starter > policy.cedar",
            "less policy.cedar",
            "hyalos policy approve policy.cedar",
        ]),
        "docs/operations/running-a-job.md",
        // As a mint reads it: a stored policy that no longer validates holds every job (ADR-0034).
        state.clone().and_then(|state| match state.policy() {
            Some(policy) => hyalos_engine::Engine::check_policy(&policy.document)
                .map(|()| "a policy is in force (hyalos policy show)".to_string())
                .map_err(|e| {
                    format!("the policy in force no longer validates ({e}): approve a new one")
                }),
            None => Err("no policy is in force, so no job can run".to_string()),
        }),
    );
    step(
        "agent_credential",
        Who::Operator,
        "in a terminal of your own: hyalos agent issue <name>, then give the client its server: claude mcp add hyalos -e HYALOS_AGENT_CREDENTIAL=<credential> -- hyalos mcp (add --harness claude-code if provider.keys has only anthropic.key)",
        vec![
            "# in the repository's directory: the credential goes straight to Claude Code, unshown. If"
                .to_string(),
            "# claude mcp add fails, hyalos agent list and hyalos agent revoke <id>, then paste again."
                .to_string(),
            format!(
                "c=$(hyalos agent issue claude-code) && claude mcp add hyalos -e HYALOS_AGENT_CREDENTIAL=\"$c\" -- hyalos mcp{}; unset c",
                // The server's jobs default to Codex, which needs OpenAI's key.
                if only_anthropic { " --harness claude-code" } else { "" }
            ),
        ],
        "docs/operations/mcp.md",
        state.and_then(|state| {
            let in_force = state
                .agents()
                .values()
                .filter(|a| a.revoked.is_none())
                .count();
            match in_force {
                0 => Err("no agent credential is in force".to_string()),
                n => Ok(format!(
                    "{n} agent credential(s) in force (hyalos agent list)"
                )),
            }
        }),
    );
    Ok(steps)
}

/// The engine's key pair, and the log against it if there is one yet (`state` is `None` before the
/// first event).
fn engine_key(
    config: &Path,
    state: Option<&Result<hyalos_engine::State, String>>,
) -> Result<String, String> {
    let key = crate::keys::load(&config.join(paths::ENGINE_KEY_FILE))
        .map_err(|e| format!("no usable engine key: {e}"))?;
    let public = std::fs::read_to_string(config.join(paths::ENGINE_PUBLIC_KEY_FILE))
        .map_err(|_| "there is no engine.pub: run hyalos init".to_string())?;
    if public.trim() != key.public().to_hex() {
        return Err("engine.pub is not engine.key's public key".into());
    }
    match state {
        Some(state) => {
            state.as_ref().map_err(Clone::clone)?;
            Ok("engine.key, engine.pub and the event log check out".to_string())
        }
        None => Ok(
            "engine.key and engine.pub check out; the event log starts with the first event written"
                .to_string(),
        ),
    }
}

fn operator_key(config: &Path) -> Result<String, String> {
    crate::operator::check(config).map_err(|e| format!("no usable operator key: {e}"))?;
    Ok("operator.key and operator.pub check out".to_string())
}

fn provider_keys(env: Env<'_>) -> Result<String, String> {
    let path = paths::provider_keys(env)?;
    if !path.exists() {
        return Err(format!("there is no {}", path.display()));
    }
    let providers = crate::broker_cmd::providers(&path)?;
    let named: Vec<&str> = [
        providers.anthropic.as_ref().map(|_| "anthropic"),
        providers.openai.as_ref().map(|_| "openai"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if named.is_empty() {
        return Err(format!("{} has no key", path.display()));
    }
    Ok(format!("a key for {}", named.join(" and ")))
}

const READY: &str = "ready: check the App can reach your repository with hyalos github check <owner/repo>, then start a job (start_job, or hyalos start <owner/repo> <issue-number>)";

/// The steps as text: each with its state, who takes it and, if not done, what to do.
pub fn text(config: &Path, steps: &[Step]) -> String {
    let done = steps.iter().filter(|s| s.done).count();
    let mut out = format!(
        "setup: {done} of {} steps done\nconfig directory: {}\n",
        steps.len(),
        config.display()
    );
    for s in steps {
        if s.done {
            out.push_str(&format!("  [done] {}: {}\n", s.id, s.detail));
        } else {
            out.push_str(&format!(
                "  [todo] {} ({}): {}\n         do: {}\n         see: {}\n",
                s.id,
                s.who.name(),
                s.detail,
                s.action,
                s.doc
            ));
        }
    }
    if done == steps.len() {
        out.push_str(READY);
        out.push('\n');
    } else {
        out.push_str(
            "hyalos setup --next prints only the next step, with its commands ready to paste\n",
        );
    }
    out
}

/// The provider key, as one POSIX shell line so a paste cannot feed a later line to its prompts. It
/// asks which provider, refuses one the file already names (the reader refuses a name given twice),
/// reads the key with echo off, and writes nothing for an empty answer, so pasting it again repairs
/// a bad attempt. `exit` leaves only the subshell.
const PROVIDER_KEY_LINE: &str = r#"(umask 077; f=@FILE@; mkdir -p @DIR@ && touch "$f" && chmod 600 "$f" || exit 1; printf 'Provider, openai or anthropic: '; read -r p; case "$p" in openai|anthropic) ;; *) echo 'not openai or anthropic, nothing written'; exit 1;; esac; if grep -qs "^[[:space:]]*$p\.key[[:space:]]*=" "$f"; then echo "$f already has $p.key: edit or remove that line first"; exit 1; fi; printf 'API key (not shown): '; stty -echo 2>/dev/null; IFS= read -r k; stty echo 2>/dev/null; echo; [ -n "$k" ] || { echo 'no key given, nothing written'; exit 1; }; printf '%s.key = %s\n' "$p" "$k" >> "$f" && echo "wrote $p.key to $f")"#;

/// The App's files, as one POSIX shell line that asks for what only the operator knows rather than
/// leaving placeholders to edit.
const GITHUB_APP_LINE: &str = r#"(umask 077; mkdir -p @DIR@ && chmod 700 @DIR@ || exit 1; printf 'App ID: '; read -r id; case "$id" in ''|*[!0-9]*) echo 'an App ID is digits only, nothing written'; exit 1;; esac; printf 'Full path of the downloaded .pem: '; read -r pem; case "$pem" in '~/'*) pem="$HOME/${pem#'~/'}";; esac; [ -f "$pem" ] || { echo "no file at $pem, nothing written"; exit 1; }; mv -- "$pem" @PEM@ && chmod 600 @PEM@ && printf 'app_id = %s\n' "$id" > @APP@ && echo 'wrote github-app.pem and github.app')"#;

/// A path as a shell word: as it is when it holds only characters a shell leaves alone, else quoted.
/// A path that is not UTF-8, or has a line break, cannot be one line of `commands`.
fn quoted(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .filter(|t| !t.contains(['\n', '\r']))
        .ok_or_else(|| {
            format!(
                "{} cannot be written in a command; set HYALOS_CONFIG_DIR to a plainer path",
                path.display()
            )
        })?;
    Ok(
        if !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%".contains(c))
        {
            text.to_string()
        } else {
            format!("'{}'", text.replace('\'', "'\\''"))
        },
    )
}

/// Only the first step not done, ending with its commands and nothing after them, so they can be
/// copied whole; or that every step is done (#258).
pub fn next(config: &Path, steps: &[Step]) -> String {
    let done = steps.iter().filter(|s| s.done).count();
    let Some(s) = steps.iter().find(|s| !s.done) else {
        return format!("setup: all {done} steps done\n{READY}\n");
    };
    let who = match s.who {
        Who::Agent => "the agent's, or yours",
        Who::Operator => "yours, in a terminal of your own",
    };
    let mut out = format!(
        "setup: {done} of {} steps done; next is {} ({who})\nconfig directory: {}\n{}\nsee {}\n\n",
        steps.len(),
        s.id,
        config.display(),
        s.detail,
        s.doc
    );
    for line in &s.commands {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The steps as JSON.
pub fn json(config: &Path, steps: &[Step]) -> Value {
    json!({
        "config_dir": config.display().to_string(),
        "done": steps.iter().filter(|s| s.done).count(),
        "total": steps.len(),
        "steps": steps.iter().map(|s| json!({
            "id": s.id,
            "done": s.done,
            "who": s.who.name(),
            "detail": s.detail,
            "action": s.action,
            "commands": s.commands,
            "doc": s.doc,
        })).collect::<Vec<_>>(),
    })
}

pub fn run(args: &[String]) -> ExitCode {
    let (as_json, args): (Vec<_>, Vec<_>) = args.iter().partition(|a| *a == "--json");
    let (only_next, args): (Vec<_>, Vec<_>) = args.into_iter().partition(|a| *a == "--next");
    let image = match args.as_slice() {
        [] => DEFAULT_IMAGE.to_string(),
        [flag, image] if flag.as_str() == "--image" => (*image).clone(),
        _ => {
            eprintln!("hyalos: setup takes only --next, --image <image> and --json\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if as_json.len() > 1 || only_next.len() > 1 {
        eprintln!("hyalos: setup takes each flag once\n{USAGE}");
        return ExitCode::from(2);
    }
    let env = |name: &str| std::env::var(name).ok();
    let substrate = crate::microvm_conf::from_env(&env).and_then(|microvm| detect(&image, microvm));
    let found = paths::config_dir(&env).and_then(|config| Ok((steps(&env, substrate)?, config)));
    let (steps, config) = match found {
        Ok(found) => found,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    match (as_json.is_empty(), only_next.is_empty()) {
        (true, true) => print!("{}", text(&config, &steps)),
        (true, false) => print!("{}", next(&config, &steps)),
        (false, true) => println!("{}", json(&config, &steps)),
        (false, false) => {
            let mut shown = json(&config, &steps);
            let first = shown["steps"]
                .as_array()
                .and_then(|all| all.iter().find(|s| s["done"] == false).cloned());
            shown["next"] = first.unwrap_or(Value::Null);
            if let Some(object) = shown.as_object_mut() {
                object.remove("steps");
            }
            println!("{shown}");
        }
    }
    if steps.iter().all(|s| s.done) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// The substrate a job would run on, asked on a thread with `hyalos run`'s timeout, since a container
/// runtime that does not answer would hang the checklist (ADR-0050).
fn detect(image: &str, microvm: Option<hyalos_substrate::MicroVmConfig>) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = image.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(hyalos_substrate::detect_with(&owned, microvm));
    });
    let timeout = crate::run_cmd::DETECT_TIMEOUT;
    let choice = rx
        .recv_timeout(timeout)
        .map_err(|_| {
            format!(
                "finding a substrate took longer than {} s, so a container runtime is not answering; check that docker info or podman info returns",
                timeout.as_secs()
            )
        })?
        .map_err(|e| e.to_string())?;
    match choice.chosen_name() {
        Some(name) => Ok(name.to_string()),
        None => Err(format!(
            "no substrate is ready for {image}; hyalos doctor says why"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::quoted;
    use std::path::Path;

    #[test]
    fn a_path_is_quoted_only_when_a_shell_would_change_it() {
        assert_eq!(
            quoted(Path::new("/home/you/.config/hyalos")),
            Ok("/home/you/.config/hyalos".into())
        );
        assert_eq!(quoted(Path::new("/a b")), Ok("'/a b'".into()));
        assert_eq!(quoted(Path::new("/it's")), Ok("'/it'\\''s'".into()));
        assert!(quoted(Path::new("/a\nb")).is_err());
    }
}
