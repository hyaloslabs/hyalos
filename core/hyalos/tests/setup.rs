//! `hyalos setup` (S5.5, ADR-0076): what a machine still needs before a first job, who takes each
//! step, and that it writes nothing and shows no secret.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use common::{EXE, GitHub, PASSPHRASE, Res, World, dir, ok_cli};
use serde_json::Value;

const IDS: [&str; 7] = [
    "engine_key",
    "operator_key",
    "substrate",
    "provider_keys",
    "github_app",
    "policy",
    "agent_credential",
];

fn todo(steps: &[hyalos::setup_cmd::Step]) -> Vec<&'static str> {
    steps.iter().filter(|s| !s.done).map(|s| s.id).collect()
}

#[test]
fn a_fresh_machine_lists_every_step_with_who_takes_it_and_setup_writes_nothing() -> Res {
    let root = dir("setup-fresh")?;
    let (config, data) = (root.join("c"), root.join("d"));
    let out = Command::new(EXE)
        .args(["setup", "--json"])
        .env("HYALOS_CONFIG_DIR", &config)
        .env("HYALOS_DATA_DIR", &data)
        .env("HYALOS_RUNTIME_DIR", root.join("r"))
        .output()?;
    assert_eq!(out.status.code(), Some(1));
    let shown: Value = serde_json::from_slice(&out.stdout)?;
    let steps = shown["steps"].as_array().ok_or("steps")?;
    let ids: Vec<&str> = steps.iter().filter_map(|s| s["id"].as_str()).collect();
    assert_eq!(ids, IDS);
    for s in steps {
        let id = s["id"].as_str().unwrap_or_default();
        // The secrets and the signed decisions are the operator's; the agent only sets up.
        let who = if ["engine_key", "substrate"].contains(&id) {
            "agent"
        } else {
            "operator"
        };
        assert_eq!(s["who"], who, "{id}");
        if id != "substrate" {
            assert_eq!(s["done"], false, "{id}");
        }
        assert!(s["action"].as_str().is_some_and(|a| !a.is_empty()));
    }
    // It wrote nothing: not the directories, not a key, not a log.
    assert!(!config.exists() && !data.exists());
    std::fs::remove_dir_all(&root).ok();
    Ok(())
}

#[test]
fn a_machine_set_up_for_a_job_has_every_step_done_once_the_agent_has_a_credential() -> Res {
    let repos = dir("setup-repos")?;
    let gh = GitHub::start(&repos, false)?;
    let world = World::new("setup", Some(&gh))?;
    let env = |name: &str| match name {
        "HYALOS_CONFIG_DIR" => Some(world.config.to_string_lossy().into_owned()),
        "HYALOS_DATA_DIR" => Some(world.data.to_string_lossy().into_owned()),
        "HYALOS_RUNTIME_DIR" => Some(world.runtime.to_string_lossy().into_owned()),
        _ => None,
    };
    let local = || Ok("local".to_string());

    // The world has keys, a policy, provider keys and the App, and no agent credential yet.
    let steps = hyalos::setup_cmd::steps(&env, local())?;
    assert_eq!(todo(&steps), ["agent_credential"]);
    let none = hyalos::setup_cmd::steps(&env, Err("no substrate is ready".into()))?;
    assert_eq!(todo(&none), ["substrate", "agent_credential"]);

    let credential = ok_cli(world.signed(&["agent", "issue", "claude-code"])?)?;
    let credential = credential.trim();
    assert!(credential.starts_with("hyalos-agent-"));
    let steps = hyalos::setup_cmd::steps(&env, local())?;
    assert!(todo(&steps).is_empty(), "{steps:?}");
    let text = hyalos::setup_cmd::text(&world.config, &steps);
    assert!(
        text.starts_with("setup: 7 of 7 steps done\nconfig directory: "),
        "{text}"
    );
    assert!(text.contains("ready: "), "{text}");

    // No secret is shown, as text, as JSON or from the command itself.
    let json = hyalos::setup_cmd::json(&world.config, &steps).to_string();
    let command = world.cli(&["setup", "--json"])?;
    let printed = String::from_utf8_lossy(&command.stdout).into_owned();
    let pem = std::fs::read_to_string(world.config.join("github-app.pem"))?;
    let pem_line = pem.lines().nth(1).ok_or("pem")?;
    for shown in [&text, &json, &printed] {
        for secret in [
            world.provider_key.as_str(),
            credential,
            PASSPHRASE,
            pem_line,
        ] {
            assert!(!shown.contains(secret), "a secret was shown: {shown}");
        }
    }
    let printed: Value = serde_json::from_str(&printed)?;
    assert_eq!(printed["total"], 7);

    // An operator key file others can read is not done, as approving would refuse it.
    let sealed = world.config.join("operator.key");
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o644))?;
    let steps = hyalos::setup_cmd::steps(&env, local())?;
    assert_eq!(todo(&steps), ["operator_key"]);
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o600))?;

    // A policy the log no longer checks out against is not taken as done.
    let public = world.config.join("engine.pub");
    std::fs::remove_file(&public)?;
    std::fs::write(&public, "00".repeat(32))?;
    let steps = hyalos::setup_cmd::steps(&env, local())?;
    assert_eq!(todo(&steps), ["engine_key", "policy", "agent_credential"]);
    Ok(())
}
