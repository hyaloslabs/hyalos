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

const USAGE: &str = "usage: hyalos setup [--image <image>] [--json]
  lists what this machine still needs before a first job, in order, and who takes each step: the
  agent (setup a shell can do) or you (a secret, or a decision signed with your approval key, which
  you run in a terminal of your own). It writes nothing and prints no secret. --image names the
  sandbox image (default hyalos-sandbox:latest); --json prints the steps as JSON.
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
    let state = log.read();
    let mut steps = Vec::new();
    let engine = engine_key(&config, &state);
    let mut step = |id, who, action, doc, found: Result<String, String>| {
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
            doc,
        });
    };

    step(
        "engine_key",
        Who::Agent,
        "hyalos init",
        "docs/operations/keys-and-recovery.md",
        engine,
    );
    step(
        "operator_key",
        Who::Operator,
        "in a terminal of your own: hyalos operator init",
        "docs/operations/approvals.md",
        operator_key(&config),
    );
    step(
        "substrate",
        Who::Agent,
        "from the repository root: docker build -f images/sandbox/Containerfile --target sandbox -t hyalos-sandbox:latest . (then hyalos doctor)",
        "docs/operations/substrates.md",
        substrate.map(|name| format!("a job would run on {name}")),
    );
    step(
        "provider_keys",
        Who::Operator,
        "write provider.keys in the config directory with a dedicated key, chmod 600 it, and set a spend cap at the provider",
        "docs/operations/provider-keys.md",
        provider_keys(env),
    );
    step(
        "github_app",
        Who::Operator,
        "create the GitHub App, save its private key as github-app.pem (chmod 600) and its id in github.app, and install it on the repository",
        "docs/operations/github-app.md",
        GitHubApp::load(&config)
            .map(|_| "github.app and github-app.pem read".to_string())
            .map_err(|e| e.to_string()),
    );
    step(
        "policy",
        Who::Operator,
        "in a terminal of your own: hyalos policy starter > policy.cedar, read it, then hyalos policy approve policy.cedar",
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

fn engine_key(
    config: &Path,
    state: &Result<hyalos_engine::State, String>,
) -> Result<String, String> {
    let key = crate::keys::load(&config.join(paths::ENGINE_KEY_FILE))
        .map_err(|e| format!("no usable engine key: {e}"))?;
    let public = std::fs::read_to_string(config.join(paths::ENGINE_PUBLIC_KEY_FILE))
        .map_err(|_| "there is no engine.pub: run hyalos init".to_string())?;
    if public.trim() != key.public().to_hex() {
        return Err("engine.pub is not engine.key's public key".into());
    }
    state.as_ref().map_err(Clone::clone)?;
    Ok("engine.key, engine.pub and the event log check out".to_string())
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
        out.push_str("ready: check the App can reach your repository with hyalos github check <owner/repo>, then start a job (start_job, or hyalos start <owner/repo> <issue-number>)\n");
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
            "doc": s.doc,
        })).collect::<Vec<_>>(),
    })
}

pub fn run(args: &[String]) -> ExitCode {
    let (as_json, args): (Vec<_>, Vec<_>) = args.iter().partition(|a| *a == "--json");
    let image = match args.as_slice() {
        [] => DEFAULT_IMAGE.to_string(),
        [flag, image] if flag.as_str() == "--image" => (*image).clone(),
        _ => {
            eprintln!("hyalos: setup takes only --image <image> and --json\n{USAGE}");
            return ExitCode::from(2);
        }
    };
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
    if as_json.is_empty() {
        print!("{}", text(&config, &steps));
    } else {
        println!("{}", json(&config, &steps));
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
