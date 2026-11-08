//! The operator's setup commands (ADR-0080): each asks for what it needs, writes it owner-only and
//! checks it, is safe to run again, and shows no secret. Answers are scripted through `Ask`.

mod common;

use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{PASSPHRASE, Res, dir};
use hyalos::setup_ask::{Ask, Setup};
use zeroize::Zeroizing;

const PEM: &[u8] = include_bytes!("fixtures/github-app-test.pem");
const KEY: &str = "sk-ant-test-key-7c41";

/// Answers given in order, and everything shown.
#[derive(Default)]
struct Script {
    answers: VecDeque<String>,
    shown: String,
}

impl Script {
    fn new(answers: &[&str]) -> Self {
        Self {
            answers: answers.iter().map(|a| a.to_string()).collect(),
            shown: String::new(),
        }
    }

    fn next(&mut self, prompt: &str) -> Result<String, String> {
        self.shown.push_str(prompt);
        self.shown.push('\n');
        self.answers
            .pop_front()
            .ok_or_else(|| format!("no answer left for {prompt:?}"))
    }
}

impl Ask for Script {
    fn say(&mut self, text: &str) {
        self.shown.push_str(text);
        self.shown.push('\n');
    }

    fn link(&mut self, before: &str, text: &str, url: &str) {
        self.shown.push_str(&format!("{before}{text} <{url}>\n"));
    }

    fn answer(&mut self, prompt: &str) -> Result<String, String> {
        self.next(prompt)
    }

    fn secret(&mut self, prompt: &str) -> Result<Zeroizing<String>, String> {
        self.next(prompt).map(Zeroizing::new)
    }
}

struct Machine {
    root: PathBuf,
    config: PathBuf,
    data: PathBuf,
    home: PathBuf,
}

impl Machine {
    fn new(name: &str) -> Res<Self> {
        let root = dir(name)?;
        let home = root.join("home");
        std::fs::create_dir_all(home.join("Downloads"))?;
        Ok(Self {
            config: root.join("c"),
            data: root.join("d"),
            home,
            root,
        })
    }

    fn env(&self) -> impl Fn(&str) -> Option<String> + '_ {
        move |name: &str| match name {
            "HYALOS_CONFIG_DIR" => Some(self.config.to_string_lossy().into_owned()),
            "HYALOS_DATA_DIR" => Some(self.data.to_string_lossy().into_owned()),
            "HOME" => Some(self.home.to_string_lossy().into_owned()),
            "USER" => Some("op".into()),
            _ => None,
        }
    }

    /// A stand-in for `claude` that records what it was asked, and fails `mcp add` when `fail` is
    /// there.
    fn claude(&self) -> Res<PathBuf> {
        let path = self.root.join("claude");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$PWD $*\" >> {0}/claude.log\nif [ \"$2\" = add ] && [ -e {0}/fail ]; then exit 1; fi\nexit 0\n",
                self.root.display()
            ),
        )?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        Ok(path)
    }

    fn mode(&self, path: &Path) -> Res<u32> {
        Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
    }
}

fn credential_in(log: &str) -> Option<String> {
    log.split_whitespace()
        .find_map(|w| w.strip_prefix("HYALOS_AGENT_CREDENTIAL="))
        .map(str::to_string)
}

#[test]
fn walk_takes_every_step_on_a_fresh_machine_and_shows_no_secret() -> Res {
    let m = Machine::new("walk")?;
    let claude = m.claude()?;
    let download = m
        .home
        .join("Downloads/hyalos-op.2026-11-08.private-key.pem");
    std::fs::write(&download, PEM)?;
    std::fs::set_permissions(&download, std::fs::Permissions::from_mode(0o644))?;
    // Another RSA key, newer, under a name that is not GitHub's: never offered, never touched.
    let other = m.home.join("mykey.pem");
    std::fs::write(&other, PEM)?;
    let mut script = Script::new(&[
        PASSPHRASE, // new operator passphrase
        PASSPHRASE, // again
        "anthropic",
        KEY,
        "",   // created the App already? no, so the browser steps
        "op", // the user name the App is named for
        "7",  // App ID
        "",   // use the key found in Downloads
        "y",  // approve the starter
        PASSPHRASE,
        "", // the directory Claude Code runs in
    ]);
    let env = m.env();
    let mut setup = Setup::new(&env, &mut script);
    setup.claude = claude.to_string_lossy().into_owned();
    setup.hyalos = "hyalos".into();
    let summary = setup.walk(Ok("local".into()))?;
    assert!(summary.starts_with("setup: 7 of 7 steps done"), "{summary}");
    assert!(script.answers.is_empty(), "{:?}", script.answers);

    // Each file is owner-only, the download was moved, and the App's link names the user.
    for file in [
        "provider.keys",
        "github-app.pem",
        "github.app",
        "operator.key",
    ] {
        assert_eq!(m.mode(&m.config.join(file))?, 0o600, "{file}");
    }
    assert_eq!(
        std::fs::read_to_string(m.config.join("provider.keys"))?,
        format!("anthropic.key = {KEY}\n")
    );
    assert_eq!(
        std::fs::read_to_string(m.config.join("github.app"))?,
        "app_id = 7\n"
    );
    assert!(!download.exists());
    assert!(other.exists() && !script.shown.contains("mykey.pem"));
    assert!(
        script
            .shown
            .contains("open github.com/settings/apps/new <https://github.com/settings/apps/new?name=hyalos-op&"),
        "{}",
        script.shown
    );
    // The passphrase was asked for once for both decisions.
    assert_eq!(script.shown.matches("\noperator passphrase: ").count(), 1);

    // claude was given the credential, the config and data directories, and Claude Code's harness,
    // since the only key is Anthropic's; in the directory this runs in.
    let log = std::fs::read_to_string(m.root.join("claude.log"))?;
    let add = log
        .lines()
        .find(|l| l.contains(" mcp add "))
        .ok_or("no add")?;
    let cwd = std::env::current_dir()?;
    assert!(
        add.starts_with(&format!("{} mcp add hyalos -e ", cwd.display())),
        "{add}"
    );
    assert!(
        add.ends_with(" -- hyalos mcp --harness claude-code"),
        "{add}"
    );
    assert!(add.contains(&format!("HYALOS_CONFIG_DIR={}", m.config.display())));
    let credential = credential_in(add).ok_or("no credential")?;
    assert!(credential.starts_with("hyalos-agent-"));

    let pem_line = std::str::from_utf8(PEM)?.lines().nth(1).ok_or("pem")?;
    for secret in [KEY, PASSPHRASE, credential.as_str(), pem_line] {
        assert!(!script.shown.contains(secret), "{secret} was shown");
        assert!(!summary.contains(secret), "{secret} was shown");
    }

    // Run again, every step is done and nothing is asked.
    let mut again = Script::new(&[]);
    let mut setup = Setup::new(&env, &mut again);
    let summary = setup.walk(Ok("local".into()))?;
    assert!(summary.starts_with("setup: 7 of 7 steps done"), "{summary}");
    std::fs::remove_dir_all(&m.root).ok();
    Ok(())
}

#[test]
fn a_key_already_in_the_config_directory_is_kept_and_made_owner_only() -> Res {
    // #274: the operator copied the key straight to github-app.pem, readable by others.
    let m = Machine::new("pem-in-place")?;
    std::fs::create_dir_all(&m.config)?;
    std::fs::set_permissions(&m.config, std::fs::Permissions::from_mode(0o700))?;
    let pem = m.config.join("github-app.pem");
    std::fs::write(&pem, PEM)?;
    std::fs::set_permissions(&pem, std::fs::Permissions::from_mode(0o775))?;
    let env = m.env();
    let step = hyalos::setup_cmd::steps(&env, Err("no substrate".into()))?
        .into_iter()
        .find(|s| s.id == "github_app")
        .ok_or("github_app")?;
    assert!(!step.done);
    assert_eq!(step.commands, ["hyalos setup github-app"]);

    // A file that is not a key writes nothing, and leaves the one in place alone.
    let wrong = m.home.join("notes.pem");
    std::fs::write(&wrong, "not a key\n")?;
    let mut script = Script::new(&["y", "7"]);
    let refused = Setup::new(&env, &mut script).github_app(Some(wrong.clone()));
    assert!(refused.is_err_and(|e| e.contains("nothing was written")));
    assert!(!m.config.join("github.app").exists() && wrong.exists());
    assert_eq!(m.mode(&pem)?, 0o775);

    // A github.app that cannot be read is refused, never overwritten.
    let settings = m.config.join("github.app");
    std::fs::create_dir(&settings)?;
    let mut script = Script::new(&[]);
    let refused = Setup::new(&env, &mut script).github_app(None);
    assert!(refused.is_err_and(|e| e.contains("cannot read")));
    std::fs::remove_dir(&settings)?;

    let mut script = Script::new(&["y", "7", ""]);
    let done = Setup::new(&env, &mut script).github_app(None)?;
    assert!(done.contains("kept the key where it was"), "{done}");
    assert!(
        script
            .shown
            .contains(&format!("Use the key {}?", pem.display())),
        "{}",
        script.shown
    );
    assert_eq!(std::fs::read(&pem)?, PEM);
    assert_eq!(m.mode(&pem)?, 0o600);
    assert_eq!(std::fs::read_to_string(&settings)?, "app_id = 7\n");

    // Given its own path, with github.app's other lines: the id is offered, the lines are kept.
    std::fs::write(
        &settings,
        "# mine\napp_id = 7\napi = https://api.github.com\n",
    )?;
    std::fs::set_permissions(&pem, std::fs::Permissions::from_mode(0o644))?;
    let mut script = Script::new(&[""]);
    Setup::new(&env, &mut script).github_app(Some(pem.clone()))?;
    assert_eq!(
        std::fs::read_to_string(&settings)?,
        "app_id = 7\n# mine\napi = https://api.github.com\n"
    );
    assert_eq!(m.mode(&pem)?, 0o600);
    let step = hyalos::setup_cmd::steps(&env, Err("no substrate".into()))?
        .into_iter()
        .find(|s| s.id == "github_app")
        .ok_or("github_app")?;
    assert!(step.done, "{step:?}");

    // Run again, it asks before replacing the App, and a no keeps it.
    let mut script = Script::new(&[""]);
    assert_eq!(
        Setup::new(&env, &mut script).github_app(None)?,
        "kept the App as it was"
    );
    std::fs::remove_dir_all(&m.root).ok();
    Ok(())
}

#[test]
fn a_provider_key_replaces_only_its_own_line_and_bad_answers_write_nothing() -> Res {
    let m = Machine::new("provider")?;
    let env = m.env();
    let file = m.config.join("provider.keys");
    let ask = |answers: &[&str]| -> Result<String, String> {
        let mut script = Script::new(answers);
        let done = Setup::new(&env, &mut script).provider_key();
        assert!(!script.shown.contains("sk-"), "{}", script.shown);
        done
    };
    for bad in [&["gemini"][..], &["openai", ""], &["openai", "sk-a b"]] {
        assert!(ask(bad).is_err_and(|e| e.contains("nothing was written")));
        assert!(!file.exists());
    }
    ask(&["openai", "sk-first"])?;
    assert_eq!(m.mode(&m.config)?, 0o700);
    assert_eq!(m.mode(&file)?, 0o600);
    // Another line the operator added is kept.
    let mut text = std::fs::read_to_string(&file)?;
    text.push_str("openai.base = http://localhost:9\n");
    std::fs::write(&file, &text)?;
    // A no keeps the key as it was; a yes replaces that line alone.
    assert_eq!(ask(&["openai", ""])?, "kept openai.key as it was");
    ask(&["openai", "y", "sk-second"])?;
    ask(&["anthropic", "sk-ant"])?;
    assert_eq!(
        std::fs::read_to_string(&file)?,
        "openai.base = http://localhost:9\nopenai.key = sk-second\nanthropic.key = sk-ant\n"
    );
    assert_eq!(m.mode(&file)?, 0o600);
    std::fs::remove_dir_all(&m.root).ok();
    Ok(())
}

#[test]
fn an_agent_credential_claude_code_refuses_is_revoked_again() -> Res {
    let m = Machine::new("agent")?;
    let claude = m.claude()?;
    let env = m.env();
    let mut script = Script::new(&[PASSPHRASE, PASSPHRASE]);
    Setup::new(&env, &mut script).operator_key()?;
    common::ok_cli(
        std::process::Command::new(common::EXE)
            .arg("init")
            .env("HYALOS_CONFIG_DIR", &m.config)
            .env("HYALOS_DATA_DIR", &m.data)
            .output()?,
    )?;
    let agents = || -> Res<usize> {
        let state = hyalos::mcp_cmd::EngineLog {
            log: m.data.join("events.db"),
            public_key: m.config.join("engine.pub"),
        }
        .read()?;
        Ok(state
            .agents()
            .values()
            .filter(|a| a.revoked.is_none())
            .count())
    };
    let run = |answers: &[&str], dir: &Path| -> Result<String, String> {
        let mut script = Script::new(answers);
        let mut setup = Setup::new(&env, &mut script);
        setup.claude = claude.to_string_lossy().into_owned();
        setup.hyalos = "hyalos".into();
        setup.agent(Some("codex".into()), Some(dir.to_path_buf()))
    };

    // Without a claude that runs, nothing is issued and nothing is asked.
    let mut script = Script::new(&[]);
    let mut setup = Setup::new(&env, &mut script);
    setup.claude = m.root.join("no-such-claude").to_string_lossy().into_owned();
    assert!(
        setup
            .agent(None, None)
            .is_err_and(|e| e.contains("does not run here"))
    );

    // claude mcp add fails: the credential just issued is revoked again.
    std::fs::write(m.root.join("fail"), "")?;
    let failed = run(&["", PASSPHRASE], &m.home);
    assert!(failed.is_err_and(|e| e.contains("was revoked again")));
    assert_eq!(agents()?, 0);
    std::fs::remove_file(m.root.join("fail"))?;

    run(&["", PASSPHRASE], &m.home)?;
    assert_eq!(agents()?, 1);
    // Again: a no keeps the one in force; a yes revokes it and issues another.
    assert_eq!(run(&["", ""], &m.home)?, "kept the credential in force");
    run(&["", "y", PASSPHRASE], &m.home)?;
    assert_eq!(agents()?, 1);
    // A replace that claude refuses leaves none in force, and says the old one went too.
    std::fs::write(m.root.join("fail"), "")?;
    let failed = run(&["", "y", PASSPHRASE], &m.home);
    assert!(failed.is_err_and(|e| e.contains("the credential it replaced was revoked")));
    assert_eq!(agents()?, 0);
    std::fs::remove_file(m.root.join("fail"))?;
    let log = std::fs::read_to_string(m.root.join("claude.log"))?;
    let adds: Vec<&str> = log.lines().filter(|l| l.contains(" mcp add ")).collect();
    assert_eq!(adds.len(), 4);
    assert!(
        adds.iter()
            .all(|a| a.starts_with(&format!("{} ", m.home.display())))
    );
    assert!(
        adds.iter()
            .all(|a| a.ends_with(" -- hyalos mcp --harness codex")),
        "{adds:?}"
    );
    // Each registration first removed the one before it in that directory.
    assert_eq!(log.matches(" mcp remove hyalos -s local").count(), 4);
    std::fs::remove_dir_all(&m.root).ok();
    Ok(())
}

#[test]
fn with_no_terminal_each_command_refuses_and_writes_nothing() -> Res {
    // setsid leaves the command with no controlling terminal, as an agent's shell may be.
    if std::process::Command::new("setsid")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: no setsid here");
        return Ok(());
    }
    let m = Machine::new("no-tty")?;
    for command in [&["walk"][..], &["provider-key"], &["policy"], &["agent"]] {
        let out = std::process::Command::new("setsid")
            .arg("-w")
            .arg(common::EXE)
            .arg("setup")
            .args(command)
            .env("HYALOS_CONFIG_DIR", &m.config)
            .env("HYALOS_DATA_DIR", &m.data)
            .stdin(std::process::Stdio::null())
            .output()?;
        assert_eq!(out.status.code(), Some(1), "{command:?}");
        assert!(out.stdout.is_empty(), "{command:?}");
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(
            said.contains("run this in a terminal of your own"),
            "{said}"
        );
        assert!(!m.config.exists() && !m.data.exists(), "{command:?}");
    }
    std::fs::remove_dir_all(&m.root).ok();
    Ok(())
}
