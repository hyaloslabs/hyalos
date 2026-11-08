//! `hyalos setup`: what still stands between this machine and a first job, step by step, and who
//! takes each step (S5.5, ADR-0076). The set-up-hyalos Agent Skill reads it to know what to do next.
//!
//! It is read-only: it writes nothing but what substrate detection caches (as `hyalos doctor` does),
//! reads a private key file only to check that it is the operator's alone and parses, makes no
//! network call, and prints no secret. Each step is the agent's, when it is
//! setup a shell can do, or the operator's, when it needs a secret (a provider key, the App's private
//! key) or is a decision signed with the operator's key behind their passphrase, which the agent must
//! never see. The operator takes those in a terminal of their own, with the short commands in
//! [`crate::setup_ask`] (`hyalos setup walk` and one per step, ADR-0080).

use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::doctor_cmd::DEFAULT_IMAGE;
use crate::github::GitHubApp;
use crate::mcp_cmd::EngineLog;
use crate::paths::{self, Env};

const USAGE: &str = "usage: hyalos setup [--next] [--image <image>] [--json]
       hyalos setup walk | operator-key | provider-key | github-app [<pem>] | policy
       hyalos setup agent [--harness codex|claude-code] [--dir <dir>]
  hyalos setup lists what this machine still needs before a first job, in order, and who takes each
  step: the agent (setup a shell can do) or you (a secret, or a decision signed with your approval
  key). It writes nothing and prints no secret. --next prints only the first step not done, ending
  with its command; --image names the sandbox image (default hyalos-sandbox:latest); --json prints
  the steps as JSON. Exit 0 when every step is done, 1 when one is not, 2 for bad usage.
  The others are yours, in a terminal of your own: each asks for what it needs there, writes it and
  checks it, and is safe to run again. walk takes every step still missing, in order";

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
    /// The commands that take it, one per line, each short enough to type (ADR-0080). A secret is
    /// read only from the terminal, never from an argument.
    pub commands: Vec<String>,
    /// Where the procedure is written down.
    pub doc: &'static str,
}

/// The steps, in the order they are best taken. `substrate` is the substrate a job would run on, or
/// why none would (from `hyalos_substrate::detect_with`, which `run` asks).
pub fn steps(env: Env<'_>, substrate: Result<String, String>) -> Result<Vec<Step>, String> {
    let config = paths::config_dir(env)?;
    let state = log_state(env)?;
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
        "in a terminal of your own: hyalos setup operator-key",
        lines(&["hyalos setup operator-key"]),
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
        "in a terminal of your own: hyalos setup provider-key, with a dedicated key that has a spend cap set at the provider",
        lines(&["hyalos setup provider-key"]),
        "docs/operations/provider-keys.md",
        provider_keys(env),
    );
    step(
        "github_app",
        Who::Operator,
        "in a terminal of your own: hyalos setup github-app, which says how to create the App and takes its key wherever it was saved",
        lines(&["hyalos setup github-app"]),
        "docs/operations/github-app.md",
        GitHubApp::load(&config)
            .map(|_| "github.app and github-app.pem read".to_string())
            .map_err(|e| format!("{e}; hyalos setup github-app sets it up")),
    );
    step(
        "policy",
        Who::Operator,
        "in a terminal of your own: hyalos setup policy, which shows the starter policy and asks before approving it",
        lines(&["hyalos setup policy"]),
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
        "in a terminal of your own, in the directory you run Claude Code in: hyalos setup agent, which issues a credential and registers the server with Claude Code without showing it",
        lines(&["hyalos setup agent"]),
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

/// The log's state as the engine's public key reads it, or `None` before the first event.
/// `hyalos init` makes the key, not the log; the first event the engine writes creates it (#255). As
/// `keys::prepare` judges it, only a path that is not there at all is no log.
pub(crate) fn log_state(
    env: Env<'_>,
) -> Result<Option<Result<hyalos_engine::State, String>>, String> {
    let log = EngineLog {
        log: paths::log_file(env)?,
        public_key: paths::engine_public_key(env)?,
    };
    Ok(match std::fs::symlink_metadata(&log.log) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(format!(
            "cannot read the event log {}: {e}",
            log.log.display()
        ))),
        Ok(_) => Some(log.read()),
    })
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
            "in a terminal of your own, hyalos setup walk takes every step still missing, asking for what each needs\n",
        );
    }
    out
}

/// Only the first step not done, ending with its command and nothing after it; or that every step
/// is done (#258).
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

/// The operator's commands, which ask in their terminal ([`crate::setup_ask`]).
const ASKING: [&str; 6] = [
    "walk",
    "operator-key",
    "provider-key",
    "github-app",
    "policy",
    "agent",
];

pub fn run(args: &[String]) -> ExitCode {
    if args.first().is_some_and(|a| ASKING.contains(&a.as_str())) {
        return crate::setup_ask::run(args);
    }
    if args.first().is_some_and(|a| a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
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
pub(crate) fn detect(
    image: &str,
    microvm: Option<hyalos_substrate::MicroVmConfig>,
) -> Result<String, String> {
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
