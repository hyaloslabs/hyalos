//! The operator's setup steps, asked in their own terminal (ADR-0080): `hyalos setup walk`, and one
//! short command per step. Each asks for what it needs, writes it owner-only, checks it, and is safe
//! to run again. Answers come through [`Ask`]: from `/dev/tty` for the operator, scripted in tests.
//! So no answer and no secret is ever in an argument, the environment or shell history, and nothing
//! the operator runs is a long line they had to copy.
//!
//! `walk`, `policy` and `agent` record decisions signed with the operator's key, so they are in
//! `tools_cmd::DECISIONS`. Nothing here prints a secret: not a provider key, the App's key, the
//! passphrase or a credential, and not what `claude` prints.

use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use hyalos_broker::sandbox::Providers;
use hyalos_engine::{Decider, Engine, State};
use hyalos_types::{AgentId, AgentName, Clock, ReasonCode};
use zeroize::Zeroizing;

use crate::agent_cmd::{credential_digest, fresh_credential};
use crate::clock::SystemClock;
use crate::github::{AppSettings, GITHUB_APP_FILE, GITHUB_KEY_FILE, GitHubApp};
use crate::keys::{self, OsSeed};
use crate::mcp_cmd::CREDENTIAL_VAR;
use crate::paths::{self, Env, OPERATOR_PUBLIC_KEY_FILE};
use crate::{operator, passphrase, policy_cmd, setup_cmd};

/// No key file read here is larger than this.
const FILE_LIMIT: u64 = 16 * 1024;

/// The name `agent` issues its credential under.
const AGENT_NAME: &str = "claude-code";

/// How setup talks to the operator.
pub trait Ask {
    /// Shows a line.
    fn say(&mut self, text: &str);
    /// Shows `before`, then `text` as a link to `url` where the terminal can follow one.
    fn link(&mut self, before: &str, text: &str, url: &str);
    /// Asks, and returns the answer as typed, trimmed.
    fn answer(&mut self, prompt: &str) -> Result<String, String>;
    /// Asks for a secret, not shown as it is typed.
    fn secret(&mut self, prompt: &str) -> Result<Zeroizing<String>, String>;
}

/// The operator's terminal: `/dev/tty` for answers, standard output for what is shown.
pub struct Terminal;

impl Ask for Terminal {
    fn say(&mut self, text: &str) {
        println!("{text}");
    }

    fn link(&mut self, before: &str, text: &str, url: &str) {
        // OSC 8: a terminal that knows it shows `text` and opens `url` on a click; one that does not
        // shows `text` alone.
        if std::io::stdout().is_terminal() {
            println!("{before}\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\");
        } else {
            println!("{before}{text}");
        }
    }

    fn answer(&mut self, prompt: &str) -> Result<String, String> {
        passphrase::answer(prompt)
    }

    fn secret(&mut self, prompt: &str) -> Result<Zeroizing<String>, String> {
        passphrase::hidden(prompt)
    }
}

/// One run of setup: where it reads and writes, how it asks, and the engine once it is unlocked.
pub struct Setup<'a> {
    env: Env<'a>,
    ask: &'a mut dyn Ask,
    /// The program `agent` registers the server with.
    pub claude: String,
    /// The command the registered server runs: this binary, so the operator's PATH does not matter.
    pub hyalos: String,
    engine: Option<Engine>,
}

impl<'a> Setup<'a> {
    pub fn new(env: Env<'a>, ask: &'a mut dyn Ask) -> Self {
        let hyalos = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
            .unwrap_or_else(|| "hyalos".to_string());
        Self {
            env,
            ask,
            claude: "claude".to_string(),
            hyalos,
            engine: None,
        }
    }

    /// Every step still missing, in order, then the checklist. The engine key is made here if it is
    /// missing; a sandbox image that is not ready stops nothing, since no other step needs it.
    pub fn walk(&mut self, substrate: Result<String, String>) -> Result<String, String> {
        let config = paths::config_dir(self.env)?;
        let ready = keys::prepare(&config, &paths::log_file(self.env)?, &mut OsSeed)
            .map_err(|e| format!("engine_key: {e}"))?;
        if ready.created {
            self.ask.say(
                "created the engine's signing key: back up engine.key (docs/operations/keys-and-recovery.md)",
            );
        }
        let ids = [
            "operator_key",
            "provider_keys",
            "github_app",
            "policy",
            "agent_credential",
        ];
        for id in ids {
            let steps = setup_cmd::steps(self.env, substrate.clone())?;
            let Some(step) = steps.iter().find(|s| s.id == id) else {
                continue;
            };
            if step.done {
                self.ask.say(&format!("[done] {id}: {}", step.detail));
                continue;
            }
            self.ask.say(&format!("\n== {id}"));
            let done = match id {
                "operator_key" => self.operator_key(),
                "provider_keys" => self.provider_key(),
                "github_app" => self.github_app(None),
                "policy" => self.policy(),
                _ => self.agent(None, None),
            }
            .map_err(|e| format!("{id}: {e}. Run hyalos setup walk again once it is fixed"))?;
            self.ask.say(&done);
        }
        let steps = setup_cmd::steps(self.env, substrate)?;
        Ok(setup_cmd::text(&config, &steps))
    }

    /// The operator's approval key, behind a passphrase they choose (ADR-0070).
    pub fn operator_key(&mut self) -> Result<String, String> {
        let config = paths::config_dir(self.env)?;
        if operator::check(&config).is_ok() {
            return Ok("your approval key is set up already (hyalos operator show)".into());
        }
        self.ask
            .say("Your approval key signs your decisions. Choose a passphrase of 12 or more");
        self.ask
            .say("characters, and never give it to the driving agent.");
        let words = self.ask.secret("new operator passphrase: ")?;
        let again = self.ask.secret("again: ")?;
        if words != again {
            return Err("the two passphrases differ; nothing was written".into());
        }
        private_dir(&config)?;
        operator::create(&config, &words, &mut OsSeed)?;
        Ok(format!(
            "created your approval key; its public half is in {}",
            config.join(OPERATOR_PUBLIC_KEY_FILE).display()
        ))
    }

    /// A provider's API key in `provider.keys`, replacing only that provider's `key` line.
    pub fn provider_key(&mut self) -> Result<String, String> {
        let config = paths::config_dir(self.env)?;
        let path = paths::provider_keys(self.env)?;
        self.ask
            .say("Use a dedicated API key, with a spend cap set at the provider. OpenAI's runs");
        self.ask
            .say("jobs with Codex, the default; Anthropic's runs them with Claude Code.");
        let provider = self.ask.answer("Provider, openai or anthropic: ")?;
        if !["openai", "anthropic"].contains(&provider.as_str()) {
            return Err("the provider is openai or anthropic; nothing was written".into());
        }
        let name = format!("{provider}.key");
        let old = if path.exists() {
            let raw = keys::read_private(&path, FILE_LIMIT).map_err(|e| e.to_string())?;
            let text = std::str::from_utf8(&raw)
                .map_err(|_| format!("{} is not text; nothing was written", path.display()))?;
            Zeroizing::new(text.to_string())
        } else {
            Zeroizing::new(String::new())
        };
        let named = |line: &str| line.split_once('=').is_some_and(|(n, _)| n.trim() == name);
        if old.lines().any(named)
            && !self.yes(
                &format!("{} has {name} already. Replace it?", path.display()),
                false,
            )?
        {
            return Ok(format!("kept {name} as it was"));
        }
        let key = self
            .ask
            .secret(&format!("{provider} API key (not shown): "))?;
        if key.is_empty() {
            return Err("no key was given; nothing was written".into());
        }
        if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("a key has no spaces or line breaks; nothing was written".into());
        }
        // Sized once, so the text holding the key never moves and leaves a copy behind.
        let mut text = Zeroizing::new(String::with_capacity(
            old.len() + name.len() + key.len() + 8,
        ));
        for line in old.lines().filter(|l| !named(l)) {
            text.push_str(line);
            text.push('\n');
        }
        text.push_str(&name);
        text.push_str(" = ");
        text.push_str(&key);
        text.push('\n');
        // Checked as the broker reads it before anything is written.
        Providers::parse(&text)
            .map_err(|e| format!("{}: {e}; nothing was written", path.display()))?;
        private_dir(&config)?;
        write_private(&path, text.as_bytes())?;
        crate::broker_cmd::providers(&path)?;
        Ok(format!("wrote {name} to {}, owner-only", path.display()))
    }

    /// The GitHub App: what to do in a browser, its id, and its private key from wherever it was
    /// saved, including the config directory itself (#274).
    pub fn github_app(&mut self, pem: Option<PathBuf>) -> Result<String, String> {
        let config = paths::config_dir(self.env)?;
        let settings_path = config.join(GITHUB_APP_FILE);
        if GitHubApp::load(&config).is_ok()
            && !self.yes(
                "The App is set up already. Set up another in its place?",
                false,
            )?
        {
            return Ok("kept the App as it was".into());
        }
        let current = fs::read_to_string(&settings_path).unwrap_or_default();
        let app_id = match AppSettings::parse(&current) {
            Ok(settings) if self.yes(&format!("Use App ID {}?", settings.app_id), true)? => {
                settings.app_id
            }
            _ => {
                if !self.yes("Have you created the App already?", false)? {
                    self.browser_steps()?;
                }
                let id = self.ask.answer("App ID: ")?;
                id.parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("an App ID is the App's number; nothing was written")?
            }
        };
        let source = match pem {
            Some(path) => path,
            None => self.find_pem(&config)?,
        };
        let raw = read_file(&source)?;
        let settings = with_app_id(&current, app_id);
        let parsed = AppSettings::parse(&settings)
            .map_err(|e| format!("{}: {e}; nothing was written", settings_path.display()))?;
        GitHubApp::new(&parsed, &raw)
            .map_err(|e| format!("{}: {e}; nothing was written", source.display()))?;
        private_dir(&config)?;
        let target = config.join(GITHUB_KEY_FILE);
        let same =
            target.exists() && fs::canonicalize(&source).ok() == fs::canonicalize(&target).ok();
        if same {
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("cannot chmod 600 {}: {e}", target.display()))?;
        } else {
            write_private(&target, &raw)?;
        }
        write_private(&settings_path, settings.as_bytes())?;
        GitHubApp::load(&config).map_err(|e| e.to_string())?;
        let from = if same {
            "kept the key where it was, owner-only".to_string()
        } else if fs::remove_file(&source).is_ok() {
            format!("moved the key from {}", source.display())
        } else {
            format!("copied the key; delete {} yourself", source.display())
        };
        Ok(format!(
            "wrote github.app and github-app.pem ({from}). Check the App can reach your repository: hyalos github check <owner/repo>"
        ))
    }

    /// What to do in a browser to create the App, in lines short enough to read across.
    fn browser_steps(&mut self) -> Result<(), String> {
        self.ask
            .say("Create the GitHub App in a browser, on any computer:");
        let user = self
            .ask
            .answer("Your GitHub user name, to name the App (Enter to skip): ")?;
        if !user.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || user.len() > 39 {
            return Err(
                "a GitHub user name is letters, digits and '-'; nothing was written".into(),
            );
        }
        let name = if user.is_empty() {
            String::new()
        } else {
            format!("name=hyalos-{user}&")
        };
        let url = format!(
            "https://github.com/settings/apps/new?{name}url=https://github.com&webhook_active=false&contents=write&issues=write&pull_requests=write"
        );
        self.ask
            .link("  1. open ", "github.com/settings/apps/new", &url);
        self.ask
            .say("     (a click fills in 2 and 3, where your terminal follows links)");
        self.ask
            .say("     for an organization: github.com/organizations/<org>/settings/apps/new");
        self.ask
            .say("  2. name hyalos-<you>, homepage https://github.com, Webhook: clear Active");
        self.ask
            .say("  3. Contents, Issues and Pull requests: Read and write; nothing else");
        self.ask
            .say("  4. Create GitHub App, note its App ID, then Generate a private key");
        self.ask
            .say("  5. Install App, on the repository the jobs will work on");
        Ok(())
    }

    /// The App's key: the newest `.pem` where a download or a copy lands, if the operator says it is
    /// the one, else a path they give, after saying how to copy it here.
    fn find_pem(&mut self, config: &Path) -> Result<PathBuf, String> {
        for _ in 0..3 {
            if let Some(found) = newest_pem(&self.places(config)) {
                if self.yes(&format!("Use the key {}?", found.display()), true)? {
                    return Ok(found);
                }
            } else {
                self.ask.say("No .pem here yet.");
            }
            let user = (self.env)("USER").unwrap_or_else(|| "you".into());
            let host = fs::read_to_string("/proc/sys/kernel/hostname")
                .map(|h| h.trim().to_string())
                .unwrap_or_else(|_| "this-machine".into());
            self.ask
                .say("If the browser saved it on another computer, copy it here from there:");
            self.ask.say(&format!(
                "  scp ~/Downloads/*.private-key.pem {user}@{host}:"
            ));
            let given = self
                .ask
                .answer("Then press Enter, or type the .pem's path: ")?;
            if !given.is_empty() {
                return Ok(match given.strip_prefix("~/") {
                    Some(rest) => self.home().join(rest),
                    None => PathBuf::from(given),
                });
            }
        }
        Err("no .pem was found or given; nothing was written".into())
    }

    fn home(&self) -> PathBuf {
        (self.env)("HOME").map(PathBuf::from).unwrap_or_default()
    }

    /// Where a key the operator downloaded or copied is likely to be.
    fn places(&self, config: &Path) -> Vec<PathBuf> {
        let home = self.home();
        let mut places = vec![config.to_path_buf(), home.join("Downloads"), home];
        // WSL2: the Windows side's downloads.
        if let Ok(users) = fs::read_dir("/mnt/c/Users") {
            places.extend(users.flatten().map(|u| u.path().join("Downloads")));
        }
        places
    }

    /// The starter policy, shown in full and approved only on a yes.
    pub fn policy(&mut self) -> Result<String, String> {
        let in_force = self
            .state()?
            .policy()
            .is_some_and(|p| Engine::check_policy(&p.document).is_ok());
        if in_force
            && !self.yes(
                "A policy is in force (hyalos policy show). Approve the starter in its place?",
                false,
            )?
        {
            return Ok("kept the policy in force".into());
        }
        self.ask.say(policy_cmd::STARTER.trim_end());
        self.ask.say("");
        self.ask
            .say("That is the starter policy (hyalos policy starter): what a first job may do.");
        if !self.yes(
            "Approve it for your jobs, signed with your approval key?",
            false,
        )? {
            return Err("not approved; nothing was written".into());
        }
        let reason = reason("policy.approved")?;
        let head = self
            .engine()?
            .approve_policy(now(), policy_cmd::STARTER, Decider::Human, reason)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "approved the starter as the policy in force from event {}",
            head.seq
        ))
    }

    /// A credential for Claude Code, registered with it for `dir` (the current directory by default)
    /// without being shown. A credential this replaces is revoked; one this issued is revoked again if
    /// registering it fails.
    pub fn agent(
        &mut self,
        harness: Option<String>,
        dir: Option<PathBuf>,
    ) -> Result<String, String> {
        let dir = match dir {
            Some(dir) => dir,
            None => std::env::current_dir()
                .map_err(|e| format!("cannot read the current directory: {e}"))?,
        };
        let runs = Command::new(&self.claude)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !runs {
            return Err(format!(
                "{} does not run here: install Claude Code, or issue a credential for another client with hyalos agent issue <name>",
                self.claude
            ));
        }
        let harness = match harness.as_deref() {
            None => {
                // Jobs default to Codex, which needs OpenAI's key.
                let only_anthropic = crate::broker_cmd::providers(&paths::provider_keys(self.env)?)
                    .is_ok_and(|p| p.anthropic.is_some() && p.openai.is_none());
                only_anthropic.then(|| "claude-code".to_string())
            }
            Some("codex" | "claude-code") => harness,
            Some(_) => return Err("the harness is codex or claude-code".into()),
        };
        let name = AgentName::new(AGENT_NAME).ok_or("bad agent name")?;
        let replaced: Vec<AgentId> = self
            .state()?
            .agents()
            .iter()
            .filter(|(_, a)| a.revoked.is_none() && a.name == name)
            .map(|(id, _)| *id)
            .collect();
        self.ask
            .say("This registers the hyalos server for Claude Code sessions started in");
        self.ask.say(&format!("  {}", dir.display()));
        if !self.yes("Is that where you run Claude Code?", true)? {
            return Err(
                "nothing was issued: run this in that directory, or with --dir <dir>".into(),
            );
        }
        if !replaced.is_empty()
            && !self.yes(
                "A claude-code credential is in force. Revoke it and issue a new one?",
                false,
            )?
        {
            return Ok("kept the credential in force".into());
        }
        let (issued, revoked) = (reason("agent.issued")?, reason("agent.revoked")?);
        let credential = fresh_credential()?;
        let engine = self.engine()?;
        for id in replaced {
            engine
                .revoke_agent(now(), id, Decider::Human, revoked.clone())
                .map_err(|e| e.to_string())?;
        }
        let head = engine
            .issue_agent(
                now(),
                name,
                credential_digest(&credential),
                Decider::Human,
                issued,
            )
            .map_err(|e| e.to_string())?;
        let quiet = |c: &mut Command| {
            c.current_dir(&dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        // One already here would make the add fail; its output may hold its credential.
        quiet(Command::new(&self.claude).args(["mcp", "remove", "hyalos", "-s", "local"]));
        let mut add = Command::new(&self.claude);
        add.args(["mcp", "add", "hyalos", "-e"]);
        add.arg(Zeroizing::new(format!("{CREDENTIAL_VAR}={}", credential.as_str())).as_str());
        for var in ["HYALOS_CONFIG_DIR", "HYALOS_DATA_DIR"] {
            if let Some(value) = (self.env)(var).filter(|v| !v.is_empty()) {
                add.arg("-e").arg(format!("{var}={value}"));
            }
        }
        add.args(["--", &self.hyalos, "mcp"]);
        if let Some(harness) = &harness {
            add.args(["--harness", harness]);
        }
        if !quiet(&mut add) {
            let agent = AgentId::from_seq(head.seq);
            let undone = self
                .engine()?
                .revoke_agent(now(), agent, Decider::Human, revoked)
                .is_ok();
            return Err(format!(
                "claude mcp add failed in {}, so {}; see claude mcp list there",
                dir.display(),
                if undone {
                    format!("agent {} was revoked again", head.seq)
                } else {
                    format!("revoke agent {} with hyalos agent revoke {0}", head.seq)
                }
            ));
        }
        Ok(format!(
            "issued agent {} (claude-code) and registered it with Claude Code for {}, without showing it. A new Claude Code session there has the tools",
            head.seq,
            dir.display()
        ))
    }

    fn yes(&mut self, question: &str, default: bool) -> Result<bool, String> {
        let hint = if default { "[Y/n]" } else { "[y/N]" };
        match self
            .ask
            .answer(&format!("{question} {hint} "))?
            .to_ascii_lowercase()
            .as_str()
        {
            "" => Ok(default),
            "y" | "yes" => Ok(true),
            "n" | "no" => Ok(false),
            _ => Err("the answer is y or n; nothing was written".into()),
        }
    }

    /// The engine with the operator's key, asking for the passphrase the first time only.
    fn engine(&mut self) -> Result<&mut Engine, String> {
        if self.engine.is_none() {
            let words = self.ask.secret("operator passphrase: ")?;
            if words.is_empty() {
                return Err("no passphrase was given".into());
            }
            self.engine = Some(operator::engine_with(self.env, &words)?);
        }
        self.engine.as_mut().ok_or_else(|| "no engine".to_string())
    }

    fn state(&self) -> Result<State, String> {
        match &self.engine {
            Some(engine) => Ok(engine.state().clone()),
            None => setup_cmd::log_state(self.env)?.unwrap_or_else(|| Ok(State::default())),
        }
    }
}

fn now() -> hyalos_types::Timestamp {
    Clock::now(&SystemClock)
}

fn reason(code: &str) -> Result<ReasonCode, String> {
    ReasonCode::new(code).ok_or_else(|| format!("bad reason {code}"))
}

/// The config directory, made owner-only if it is not there yet, and refused if others can reach it.
fn private_dir(config: &Path) -> Result<(), String> {
    if !config.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(config)
            .map_err(|e| format!("cannot make {}: {e}", config.display()))?;
    }
    keys::private_dir(config).map_err(|e| e.to_string())
}

/// `bytes` at `path`, owner-only, through a new file renamed over it, so no reader sees half a file.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} is not a file name", path.display()))?;
    let temporary = path.with_file_name(format!(".{name}.new"));
    let _ = fs::remove_file(&temporary);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| fs::rename(&temporary, path));
    written.map_err(|e| {
        let _ = fs::remove_file(&temporary);
        format!("cannot write {}: {e}", path.display())
    })
}

/// A key file the operator gave or setup found: a regular file, at most [`FILE_LIMIT`] bytes.
fn read_file(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("cannot read {}: {e}; nothing was written", path.display()))?;
    if !meta.is_file() {
        return Err(format!(
            "{} is not a file; nothing was written",
            path.display()
        ));
    }
    let mut raw = Zeroizing::new(Vec::with_capacity(FILE_LIMIT as usize));
    fs::File::open(path)
        .and_then(|f| f.take(FILE_LIMIT).read_to_end(&mut raw))
        .map_err(|e| format!("cannot read {}: {e}; nothing was written", path.display()))?;
    Ok(raw)
}

/// `github.app` with `app_id` set, keeping its other lines.
fn with_app_id(current: &str, app_id: u64) -> String {
    let mut text = format!("app_id = {app_id}\n");
    for line in current.lines() {
        if line
            .split_once('=')
            .is_none_or(|(n, _)| n.trim() != "app_id")
        {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// The `.pem` most likely to be the App's key in `places`: GitHub names its downloads
/// `<app>.<date>.private-key.pem`, so those come first, then the newest. Only the operator's own
/// regular files, not links.
fn newest_pem(places: &[PathBuf]) -> Option<PathBuf> {
    let user = rustix::process::getuid().as_raw();
    places
        .iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            let meta = fs::symlink_metadata(&path).ok()?;
            let ours = meta.is_file() && meta.uid() == user && meta.len() <= FILE_LIMIT;
            (ours && name.ends_with(".pem")).then(|| {
                let github = name.ends_with(".private-key.pem") || name == GITHUB_KEY_FILE;
                ((github, meta.mtime()), path)
            })
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}

pub fn run(args: &[String]) -> ExitCode {
    // Before anything is shown or written, so a shell with no terminal gets one line.
    if let Err(e) = passphrase::has_terminal() {
        eprintln!("hyalos: {e}");
        return ExitCode::from(1);
    }
    let env = |name: &str| std::env::var(name).ok();
    let mut terminal = Terminal;
    let mut setup = Setup::new(&env, &mut terminal);
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match words.as_slice() {
        ["walk"] => {
            let substrate = crate::microvm_conf::from_env(&env)
                .and_then(|microvm| setup_cmd::detect(crate::doctor_cmd::DEFAULT_IMAGE, microvm));
            setup.walk(substrate)
        }
        ["operator-key"] => setup.operator_key(),
        ["provider-key"] => setup.provider_key(),
        ["github-app"] => setup.github_app(None),
        ["github-app", pem] => setup.github_app(Some(PathBuf::from(pem))),
        ["policy"] => setup.policy(),
        ["agent", rest @ ..] => match agent_args(rest) {
            Ok((harness, dir)) => setup.agent(harness, dir),
            Err(e) => {
                eprintln!("hyalos: {e}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("hyalos: unknown setup command; see hyalos setup --help");
            return ExitCode::from(2);
        }
    };
    match done {
        Ok(text) => {
            println!("{}", text.trim_end());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hyalos: {e}");
            ExitCode::from(1)
        }
    }
}

fn agent_args(args: &[&str]) -> Result<(Option<String>, Option<PathBuf>), String> {
    let (mut harness, mut dir) = (None, None);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or(format!("{flag} takes a value"))?;
        let slot = match *flag {
            "--harness" => harness.replace(value.to_string()).is_some(),
            "--dir" => dir.replace(PathBuf::from(value)).is_some(),
            _ => return Err("agent takes only --harness <h> and --dir <dir>".into()),
        };
        if slot {
            return Err(format!("{flag} is given twice"));
        }
    }
    Ok((harness, dir))
}
