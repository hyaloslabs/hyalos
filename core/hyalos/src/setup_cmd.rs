//! `hyalos setup`: what still stands between this machine and a first job, step by step, and who
//! takes each step (S5.5, ADR-0076). The set-up-hyalos Agent Skill reads it to know what to do next.
//!
//! It is read-only: it writes nothing, opens no private key's contents beyond checking that the file
//! is the operator's alone and parses, and prints no secret. Each step is the agent's, when it is
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
        engine_key(&config, &log),
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
        "from the repository root: docker build -f images/sandbox/Containerfile -t hyalos-sandbox:latest . (then hyalos doctor)",
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
        "hyalos policy starter > policy.cedar, read it, then in a terminal of your own: hyalos policy approve policy.cedar",
        "docs/operations/running-a-job.md",
        state.clone().and_then(|state| match state.policy() {
            Some(_) => Ok("a policy is in force (hyalos policy show)".to_string()),
            None => Err("no policy is in force, so no job can run".to_string()),
        }),
    );
    step(
        "agent_credential",
        Who::Operator,
        "in a terminal of your own: hyalos agent issue <name>, then give the client its server: claude mcp add hyalos -e HYALOS_AGENT_CREDENTIAL=<credential> -- hyalos mcp",
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

fn engine_key(config: &Path, log: &EngineLog) -> Result<String, String> {
    crate::keys::load(&config.join(paths::ENGINE_KEY_FILE))
        .map_err(|e| format!("no usable engine key: {e}"))?;
    log.read()?;
    Ok("engine.key, engine.pub and the event log check out".to_string())
}

fn operator_key(config: &Path) -> Result<String, String> {
    crate::operator::read_public(&config.join(paths::OPERATOR_PUBLIC_KEY_FILE))
        .map_err(|e| format!("no operator key yet: {e}"))?;
    if !config.join(paths::OPERATOR_KEY_FILE).is_file() {
        return Err("operator.pub is there but operator.key is not".into());
    }
    Ok("operator.key and operator.pub are there".to_string())
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
pub fn text(steps: &[Step]) -> String {
    let done = steps.iter().filter(|s| s.done).count();
    let mut out = format!("setup: {done} of {} steps done\n", steps.len());
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
pub fn json(steps: &[Step]) -> Value {
    json!({
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
    let substrate = crate::microvm_conf::from_env(&env).and_then(|microvm| {
        hyalos_substrate::detect_with(&image, microvm)
            .map_err(|e| format!("{e}"))
            .and_then(|choice| match choice.chosen_name() {
                Some(name) => Ok(name.to_string()),
                None => Err(format!(
                    "no substrate is ready for {image}; hyalos doctor says why"
                )),
            })
    });
    let steps = match steps(&env, substrate) {
        Ok(steps) => steps,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    if as_json.is_empty() {
        print!("{}", text(&steps));
    } else {
        println!("{}", json(&steps));
    }
    if steps.iter().all(|s| s.done) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
