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

#[test]
fn hyalos_init_alone_clears_the_engine_key_step_before_any_log_exists() -> Res {
    let root = dir("setup-init")?;
    let (config, data) = (root.join("c"), root.join("d"));
    let run = |args: &[&str]| {
        Command::new(EXE)
            .args(args)
            .env("HYALOS_CONFIG_DIR", &config)
            .env("HYALOS_DATA_DIR", &data)
            .env("HYALOS_RUNTIME_DIR", root.join("r"))
            .output()
    };
    ok_cli(run(&["init"])?)?;
    // init makes the key pair and no log (#255).
    assert!(!data.join("events.db").exists());
    let env = |name: &str| match name {
        "HYALOS_CONFIG_DIR" => Some(config.to_string_lossy().into_owned()),
        "HYALOS_DATA_DIR" => Some(data.to_string_lossy().into_owned()),
        _ => None,
    };
    let steps = hyalos::setup_cmd::steps(&env, Err("no substrate".into()))?;
    let engine = steps
        .iter()
        .find(|s| s.id == "engine_key")
        .ok_or("engine_key")?;
    assert!(engine.done, "{engine:?}");
    // With no log the state is empty, so the later steps say what is missing, whichever comes first.
    for (id, missing) in [
        ("policy", "no policy is in force"),
        ("agent_credential", "no agent credential is in force"),
    ] {
        let step = steps.iter().find(|s| s.id == id).ok_or(id)?;
        assert!(!step.done && step.detail.contains(missing), "{step:?}");
    }
    // Running init again changes nothing, and setup still writes nothing.
    let keys = |dir: &std::path::Path| -> Res<(Vec<u8>, Vec<u8>)> {
        Ok((
            std::fs::read(dir.join("engine.key"))?,
            std::fs::read(dir.join("engine.pub"))?,
        ))
    };
    let before = keys(&config)?;
    ok_cli(run(&["init"])?)?;
    assert_eq!(keys(&config)?, before);
    let out = run(&["setup", "--json"])?;
    assert_eq!(out.status.code(), Some(1));
    let shown: Value = serde_json::from_slice(&out.stdout)?;
    let engine = shown["steps"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["id"] == "engine_key"))
        .ok_or("engine_key")?;
    assert_eq!(engine["done"], true, "{engine}");
    assert!(!data.join("events.db").exists());

    // A log path that is there but cannot be read, such as a dangling link, is not taken as no log.
    std::os::unix::fs::symlink(root.join("nowhere.db"), data.join("events.db"))?;
    let steps = hyalos::setup_cmd::steps(&env, Err("no substrate".into()))?;
    let engine = steps
        .iter()
        .find(|s| s.id == "engine_key")
        .ok_or("engine_key")?;
    assert!(!engine.done, "{engine:?}");
    std::fs::remove_dir_all(&root).ok();
    Ok(())
}

#[test]
fn setup_next_prints_only_the_next_step_with_its_commands_last() -> Res {
    let root = dir("setup next")?;
    let (config, data) = (root.join("c"), root.join("d"));
    let run = |args: &[&str]| {
        Command::new(EXE)
            .args(args)
            .env("HYALOS_CONFIG_DIR", &config)
            .env("HYALOS_DATA_DIR", &data)
            .env("HYALOS_RUNTIME_DIR", root.join("r"))
            .output()
    };
    ok_cli(run(&["init"])?)?;
    let out = run(&["setup", "--next"])?;
    assert_eq!(out.status.code(), Some(1));
    let shown = String::from_utf8_lossy(&out.stdout).into_owned();
    // The engine key is done, so the operator key is next, and nothing of any other step is shown.
    assert!(
        shown.contains("next is operator_key (yours, in a terminal of your own)"),
        "{shown}"
    );
    for other in ["provider_keys", "github_app", "policy", "agent_credential"] {
        assert!(!shown.contains(other), "{shown}");
    }
    assert!(shown.ends_with("\n\nhyalos operator init\n"), "{shown}");

    // The JSON form carries the one step, with its commands, and no list.
    let out = run(&["setup", "--next", "--json"])?;
    assert_eq!(out.status.code(), Some(1));
    let shown: Value = serde_json::from_slice(&out.stdout)?;
    assert_eq!(shown["next"]["id"], "operator_key");
    assert_eq!(
        shown["next"]["commands"],
        serde_json::json!(["hyalos operator init"])
    );
    assert!(shown.get("steps").is_none());
    assert_eq!(run(&["setup", "--next", "--next"])?.status.code(), Some(2));

    // The provider key step's commands, pasted into a shell with the key typed in, take the step,
    // in a config directory whose path needs quoting.
    let env = |name: &str| match name {
        "HYALOS_CONFIG_DIR" => Some(config.to_string_lossy().into_owned()),
        "HYALOS_DATA_DIR" => Some(data.to_string_lossy().into_owned()),
        _ => None,
    };
    let steps = hyalos::setup_cmd::steps(&env, Err("no substrate".into()))?;
    let keys = steps
        .iter()
        .find(|s| s.id == "provider_keys")
        .ok_or("keys")?;
    assert!(!keys.done);
    let script = keys.commands.join("\n");
    assert!(!script.contains("sk-"), "{script}");
    let mut shell = Command::new("bash")
        .args(["-c", &script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    use std::io::Write;
    shell
        .stdin
        .take()
        .ok_or("stdin")?
        .write_all(b"sk-test-key-0123\n")?;
    assert!(shell.wait()?.success());
    let file = config.join("provider.keys");
    assert_eq!(
        std::fs::metadata(&file)?.permissions().mode() & 0o777,
        0o600
    );
    let steps = hyalos::setup_cmd::steps(&env, Err("no substrate".into()))?;
    assert!(
        steps.iter().any(|s| s.id == "provider_keys" && s.done),
        "{steps:?}"
    );

    // Every step has commands; with every step done, --next says so and shows none.
    assert!(steps.iter().all(|s| !s.commands.is_empty()));
    let mut all = steps.clone();
    for s in &mut all {
        s.done = true;
    }
    let shown = hyalos::setup_cmd::next(&config, &all);
    assert!(
        shown.starts_with("setup: all 7 steps done\nready: "),
        "{shown}"
    );
    std::fs::remove_dir_all(&root).ok();
    Ok(())
}
