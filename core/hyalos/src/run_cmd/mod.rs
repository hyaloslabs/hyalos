//! `hyalos run`: the reference job from the command line (S3.10, ADR-0050; S4.6, ADR-0062).
//!
//! The operator gives a repository and either their own text (`--issue`), which one actor job works
//! on, or an issue's number on GitHub (`--issue-number`), which runs the shipped fix-issue workflow:
//! a reader and a planner, each a job the broker runs that returns typed values, then the actor.
//! Every job belongs to one task and runs after the one before it ends. Only the reader sees the
//! issue's text; the actor gets the operator's text and typed values. With `--release-issue` the
//! broker also holds the issue as a handle, the operator's release of it is recorded against its
//! digest, and the actor may read that text from the broker (ADR-0063).
//!
//! For the actor the engine finds a ready substrate, records the job (with its substrate and
//! credential tier), signs a manifest the policy allows, mints a GitHub token for that one
//! repository, and starts the sandbox. The harness clones, pushes its branch and opens a pull
//! request, all through the job's socket; no credential enters the sandbox. While it runs, every gate
//! answer the broker reports, including egress refusals it sends unprompted, is recorded as
//! `gate.answered`. When the sandbox ends the engine ends the job in the broker, revokes the token,
//! and records how it ended.
//!
//! The files follow a run's phases: `parse.rs` reads the command line, `plan.rs` decides what runs
//! and writes the actor's prompt, `drive.rs` chooses the substrate and runs the jobs in turn,
//! `step.rs` runs a reader or a planner, `release.rs` holds the issue for the operator's release,
//! `actor.rs` runs the sandbox, and `report.rs` reads what the broker says and ends the job. This file
//! holds the command's entry point, what the others share, and the tests.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use hyalos_engine::{Decider, Engine, Ruling, TaskStatus};
use hyalos_policy::{GateRequest, Outcome as GateOutcome};
use hyalos_substrate::{
    Choice, Ending, HOST_OS, Outcome, SKILLS_IN_SANDBOX, SandboxSpec, Substrate, Unsupported,
    detect_with,
};
use hyalos_types::wire::{MAX_PLUGIN_INPUT, PluginCall, Protocol, ToBroker, ToEngine};
use hyalos_types::workflow::{Output, Step, TypedValue, values_fit};
use hyalos_types::{
    ActorPrompt, Answer, Attestation, Budget, Clock, CredentialTier, Digest, Event, ForgeToken,
    Grant, HoldReason, JobId, JobOutcome, MANIFEST_VERSION, Manifest, ManifestKey, OperatorText,
    Origin, PluginName, ReasonCode, Role, SignedManifest, Stakes, StepInputs, SubstrateName,
    TaskId, Timestamp, flow,
};

use hyalos_plugin::skill::{Skill, Store};

use crate::clock::SystemClock;
use crate::doctor_cmd::{self, DEFAULT_IMAGE};
use crate::github::GitHubApp;
use crate::interrupt::{self, Interrupt};
use crate::keys;
use crate::launch::{BrokerProcess, Incoming, JobDir, fresh_manifest_key};
use crate::paths::{self, Env};
use crate::workflow_run::{self, Runnable, Supplied, Values};

mod actor;
mod drive;
mod parse;
mod plan;
mod release;
mod report;
mod step;
pub mod why;

use self::{plan::*, release::*, report::*};
pub use drive::{resume_job, run_job};
pub use parse::{
    STORED_VERSION, Stored, check, is_branch, is_repo, parse, request_digest, request_digest_of,
};
pub use report::attestation_text;

const USAGE: &str = "usage: hyalos run --repo <owner/name>
                  (--issue <text> | --issue-file <file> | --issue-number <n>) [--base <branch>] [--harness codex|claude-code] [--image <image>]
                  [--budget-tokens <n>] [--minutes <n>] [--allow-host <host>]... [--step-model <model>]
                  [--release-issue [--passphrase-stdin] | --await-release] [--skill <name>]... [--workflow <file>]
  runs a harness in a sandbox on <owner/name>: it pushes the branch hyalos/job-<id> and opens a
  pull request into --base (default main). With --issue or --issue-file it works on your text.
  --issue-number runs the fix-issue workflow on issue <n> of <owner/name>: a reader in the broker
  classifies the issue, a planner chooses an approach, and the harness gets those typed values and
  your text (--issue, if given) but not the issue's own text, unless you give --release-issue: you
  then release the issue as it is when the reader holds it, by its digest, and the harness may read
  that text from the broker with hyalos-handle. The release is your decision, signed with your
  approval key: the passphrase is asked for first, or read from stdin with --passphrase-stdin.
  --await-release holds the issue the same way and waits, before the harness starts, until you run
  hyalos release <task> from another terminal (the run's deadline ends the wait). Readers and planners call
  --step-model (default a small model of the harness's provider). It needs hyalos init, an approved
  policy that allows read, push, open_pr and data_flow (hyalos policy approve), provider.keys and
  the GitHub App (github.app, github-app.pem) in the config directory, and a ready substrate (hyalos
  doctor). --allow-host lets the job reach a web host through the egress gate, if policy allows it.
  --skill gives the harness an installed skill (hyalos plugin install), read-only in its sandbox; the
  job may then do only what the skill was granted as well (ADR-0089). --workflow runs your workflow
  document instead of the fix-issue workflow, checked as hyalos workflow check checks it; a
  deterministic step runs its installed plugin's component in the broker, and each plugin a step
  names, or whose output reaches it, bounds it by its grant (ADR-0102).
  Defaults: harness codex, image hyalos-sandbox:latest (or the one a release names by digest),
  200000 tokens, 30 minutes";

/// How long finding a substrate may take. A runtime that does not answer refuses the job rather than
/// hanging it (ADR-0043, ADR-0050).
pub const DETECT_TIMEOUT: Duration = Duration::from_secs(60);

/// What a run's actor may do on the forge. Its readers ask only `read`, a subset, so these decide every
/// permission a run's tokens need; `hyalos github check` asks for each (#256).
pub const ACTOR_ACTIONS: [&str; 3] = ["read", "push", "open_pr"];
/// How long the broker may take to start and to answer the engine.
const BROKER_WAIT: Duration = Duration::from_secs(20);
/// How often the engine looks up from the broker's channel to see whether the sandbox has ended.
const POLL: Duration = Duration::from_millis(200);
/// The longest the engine waits for a reader or planner step's values. The relay's retries fit in it.
const STEP_WAIT: Duration = Duration::from_secs(180);
/// The most tokens one reader or planner step may spend, within `--budget-tokens`.
pub const STEP_BUDGET_TOKENS: u64 = 32_000;

/// How long the run waits for the broker to answer with a job's submitted result. The broker has it
/// already; this bounds a broker that does not answer.
const RESULT_WAIT: Duration = Duration::from_secs(10);
/// The model a reader or planner calls without `--step-model`, by the harness's provider.
pub const DEFAULT_OPENAI_STEP_MODEL: &str = "gpt-5-mini";
pub const DEFAULT_ANTHROPIC_STEP_MODEL: &str = "claude-haiku-5-5";
/// Where a job reaches its repository, through the sandbox's forwarder (ADR-0046, ADR-0049).
const FORWARDER: &str = "http://127.0.0.1:8089";

/// What the operator asked for.
#[derive(Debug, Clone)]
pub struct Request {
    pub repo: String,
    /// The operator's text: the task, or with [`Request::issue_number`] the operator's description
    /// of the work, which may be empty.
    pub issue: String,
    /// The issue the fix-issue workflow works on (ADR-0062).
    pub issue_number: Option<u64>,
    pub base: String,
    pub image: String,
    pub harness: Harness,
    pub budget_tokens: u64,
    pub minutes: u64,
    pub egress_hosts: BTreeSet<String>,
    /// The model readers and planners call, instead of the default for the harness's provider.
    pub step_model: Option<String>,
    /// The operator releases the issue's text to the actor (ADR-0063): the broker holds it as a
    /// handle and the release is recorded against its digest. Needs [`Request::issue_number`].
    pub release_issue: bool,
    /// The run holds the issue as a handle, as with [`Request::release_issue`], and the actor's job
    /// waits until the operator releases it with `hyalos release <task>` (ADR-0078). What `start_job`
    /// and `hyalos start` ask for, since the driving agent never holds the operator's key.
    pub await_release: bool,
    /// The MCP server's start nonce (ADR-0069), so each run it starts has a request of its own and
    /// [`request_digest`] names exactly one task. `None` from the command line.
    pub nonce: Option<String>,
    /// The installed skills the actor is given (ADR-0089): their files, read-only in its sandbox,
    /// and their grants, which cut its manifest.
    pub skills: BTreeSet<PluginName>,
    /// A workflow document the operator wrote, run instead of the fix-issue workflow (ADR-0102).
    pub workflow: Option<String>,
}

/// The program the sandbox runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Harness {
    Codex,
    ClaudeCode,
    /// A command of the caller's, given the prompt as its last argument. For tests.
    Command(Vec<String>),
}

impl Harness {
    /// The sandbox's command: the harness, given the prompt, with the MCP servers of `skills`
    /// registered so it can call them (ADR-0101). Each runs in the sandbox, from the skill's
    /// read-only directory, with the sandbox's proxy, so its only way out is the broker's egress gate.
    fn command(&self, prompt: String, skills: &[(Skill, Grant)]) -> Vec<String> {
        let servers: Vec<(&str, String, Vec<String>)> = skills
            .iter()
            .filter_map(|(skill, _)| {
                let name = skill.name.as_str();
                let root = format!("{SKILLS_IN_SANDBOX}/{name}");
                let (command, args) = skill.mcp.as_ref()?.launch(&root);
                Some((name, command, args))
            })
            .collect();
        let proxy = ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"];
        let mut command: Vec<String> = match self {
            Harness::Codex => {
                let mut command: Vec<String> =
                    ["hyalos-forward", "codex", "exec", "--skip-git-repo-check"]
                        .map(String::from)
                        .to_vec();
                // Codex's `-c` takes TOML, and a JSON string or list of strings is TOML too.
                let env: Vec<String> = proxy
                    .iter()
                    .map(|k| format!("{k} = \"{FORWARDER}\""))
                    .chain(["NO_PROXY = \"127.0.0.1,localhost\"".into()])
                    .collect();
                for (name, program, args) in &servers {
                    for (key, value) in [
                        (
                            "command",
                            serde_json::Value::from(program.as_str()).to_string(),
                        ),
                        ("args", serde_json::Value::from(args.clone()).to_string()),
                        ("env", format!("{{ {} }}", env.join(", "))),
                    ] {
                        command.push("-c".into());
                        command.push(format!("mcp_servers.{name}.{key}={value}"));
                    }
                }
                command
            }
            Harness::ClaudeCode => {
                // Only the job's servers, never one from a project file the actor finds.
                let mut command: Vec<String> = [
                    "hyalos-forward",
                    "claude",
                    "-p",
                    &prompt,
                    "--tools",
                    "Bash",
                    "--strict-mcp-config",
                ]
                .map(String::from)
                .to_vec();
                if !servers.is_empty() {
                    let mut env: serde_json::Map<String, serde_json::Value> = proxy
                        .iter()
                        .map(|k| (k.to_string(), FORWARDER.into()))
                        .collect();
                    env.insert("NO_PROXY".into(), "127.0.0.1,localhost".into());
                    let config: serde_json::Map<String, serde_json::Value> = servers
                        .iter()
                        .map(|(name, program, args)| {
                            let server = serde_json::json!({
                                "type": "stdio", "command": program, "args": args, "env": env,
                            });
                            (name.to_string(), server)
                        })
                        .collect();
                    let config = serde_json::json!({ "mcpServers": config });
                    command.extend(["--mcp-config".into(), config.to_string()]);
                }
                // Each server's tools, by Claude Code's name for them, beside the shell.
                command.extend(["--allowedTools".into(), "Bash".into()]);
                command.extend(servers.iter().map(|(name, ..)| format!("mcp__{name}")));
                return command;
            }
            Harness::Command(command) => command.clone(),
        };
        command.push(prompt);
        command
    }
}

/// Finds the substrates on a host. [`detect_with`] in the binary; tests pass another.
pub type Detect = Box<dyn FnOnce(&str) -> Result<Choice, Unsupported> + Send>;

/// Where the engine finds what it needs.
pub struct Setup {
    pub config_dir: PathBuf,
    pub log: PathBuf,
    /// Where the broker's channel and the job's socket are made.
    pub runtime_dir: PathBuf,
    /// The `hyalos` executable the broker is started from.
    pub broker_exe: PathBuf,
    pub detect: Detect,
    pub detect_timeout: Duration,
    /// Whether the operator has interrupted the run (ADR-0053). [`Interrupt::none`] in most tests.
    pub interrupt: Interrupt,
    /// Called once the task is recorded, before any job: the MCP runner names its files after the
    /// task and tells the server its id (ADR-0069). It returns a file whose appearance cancels the run.
    pub on_task: Option<OnTask>,
    /// The operator's key, unlocked before the run for `--release-issue`, the one human decision a run
    /// records (ADR-0070). A run the MCP server starts never has it.
    pub operator: Option<hyalos_types::OperatorKey>,
}

/// See [`Setup::on_task`].
pub type OnTask = Box<dyn FnOnce(TaskId) -> Result<Option<PathBuf>, String> + Send>;

/// How the job went.
#[derive(Debug, PartialEq, Eq)]
pub struct Summary {
    pub job: JobId,
    pub outcome: JobOutcome,
    /// Whether the forge token was revoked when the job ended. A failure is the operator's to act on.
    pub revoked: Result<(), String>,
}

impl Summary {
    /// Succeeded only if the job succeeded and its token is gone.
    pub fn ok(&self) -> bool {
        self.outcome == JobOutcome::Succeeded && self.revoked.is_ok()
    }
}

pub fn run(args: &[String]) -> ExitCode {
    // The two ways the MCP server starts a run, and the operator's resume (ADR-0069).
    match args {
        [flag, nonce] if flag == "--mcp-start" => return crate::runs::start(nonce),
        [flag, task] if flag == "--resume" => return crate::runs::resume(task, false),
        [flag, task, auto] if flag == "--resume" && auto == "--auto" => {
            return crate::runs::resume(task, true);
        }
        _ => {}
    }
    let (args, source) = crate::passphrase::source(args);
    let request = match parse(&args) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let env = |name: &str| std::env::var(name).ok();
    let mut setup = match setup(&env) {
        Ok(setup) => setup,
        Err(message) => {
            eprintln!("hyalos: {message}");
            return ExitCode::from(2);
        }
    };
    // The release is the operator's decision: the key is opened before anything is written.
    if request.release_issue {
        let key = crate::passphrase::read(source, "operator passphrase: ")
            .and_then(|words| crate::operator::unlock(&setup.config_dir, &words));
        match key {
            Ok(key) => setup.operator = Some(key),
            Err(message) => {
                eprintln!("hyalos: --release-issue: {message}");
                return ExitCode::from(1);
            }
        }
    }
    match run_job(&request, setup, &mut std::io::stdout()) {
        Ok(summary) if summary.ok() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(message) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(1)
        }
    }
}

pub(crate) fn setup(env: Env<'_>) -> Result<Setup, String> {
    Ok(Setup {
        config_dir: paths::config_dir(env)?,
        log: paths::log_file(env)?,
        runtime_dir: paths::runtime_dir(env)?,
        broker_exe: std::env::current_exe()
            .map_err(|e| format!("cannot find this executable: {e}"))?,
        detect: {
            let microvm = crate::microvm_conf::from_env(env)?;
            Box::new(move |image: &str| detect_with(image, microvm))
        },
        detect_timeout: DETECT_TIMEOUT,
        // From here the first Ctrl-C or SIGTERM ends the job cleanly, and a second exits at once.
        interrupt: Interrupt::on_signals(again)
            .map_err(|e| format!("cannot take Ctrl-C and SIGTERM: {e}"))?,
        on_task: None,
        operator: None,
    })
}

/// A second signal: stop at once, and say what that leaves undone (ADR-0053).
fn again(signal: i32) {
    eprintln!(
        "{}",
        again_message(interrupt::name(signal).unwrap_or("a signal"))
    );
    std::process::exit(130);
}

/// What a second interrupt prints. A test keeps runs of spaces out of it.
fn again_message(signal: &str) -> String {
    format!(
        "hyalos: interrupted again ({signal}), so stopping now without finishing. The job's \
         GitHub token may still be live: revoke it on GitHub (the App's installation tokens \
         expire within an hour); see docs/operations/github-app.md. The log does not record how \
         the job ended, and its sandbox may still be running until its deadline"
    )
}

/// A clock that never goes back, since the log refuses an event stamped before the one before it.
struct Stamps(Timestamp);

impl Stamps {
    fn now(&mut self) -> Timestamp {
        self.0 = self.0.max(SystemClock.now());
        self.0
    }
}

/// One of the [`why`] codes. A test checks every one is a valid code.
fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

/// Why a step failed: the code `job.ended` records, and the words the operator reads.
struct Failure {
    reason: ReasonCode,
    why: String,
}

impl Failure {
    fn new(reason: &str, why: impl Into<String>) -> Self {
        Self {
            reason: code(reason),
            why: why.into(),
        }
    }
}

/// The failure an interrupt is, once one has come. It also tells the operator what happens now.
fn interrupted(interrupt: &Interrupt, job: JobId, out: &mut dyn Write) -> Option<Failure> {
    let signal = interrupt.signal()?;
    let _ = writeln!(
        out,
        "job {}: interrupted ({signal}): ending the job and revoking its token. Interrupt again to \
         stop at once, which can leave the token live",
        job.seq()
    );
    let reason = if signal == interrupt::CANCELLED {
        why::CANCELLED
    } else {
        why::INTERRUPTED
    };
    Some(Failure::new(reason, format!("interrupted ({signal})")))
}

/// What a resumed run found already done (ADR-0069).
#[derive(Default)]
struct Done {
    /// The values of the steps that finished, in order.
    values: Values,
    /// How many steps finished, the actor included.
    steps: usize,
    /// The last job's ending, if the run is over: it did not succeed, or every step finished.
    over: Option<Summary>,
}

/// What the jobs of one run share.
struct Run<'a> {
    request: &'a Request,
    engine: Engine,
    clock: Stamps,
    task: hyalos_types::TaskId,
    app: GitHubApp,
    provider_keys: PathBuf,
    runtime_dir: PathBuf,
    broker_exe: PathBuf,
    manifest_key: ManifestKey,
    deadline: Timestamp,
    interrupt: Interrupt,
    /// Started for the first job that needs it, and shared by the rest.
    broker: Option<BrokerProcess>,
    /// A token an earlier job could not revoke, which the run reports however it ends.
    revoked: Result<(), String>,
    /// The issue's handle, once the broker holds it and the operator's release is recorded.
    handle: Option<Digest>,
    /// The issue's handle while it waits for the operator's release, and its number (ADR-0078).
    awaiting: Option<(Digest, u64)>,
    /// Where an awaited handle is written for `hyalos release` (`runs/task-<id>.held`).
    held_file: PathBuf,
    /// The skills the actor is given ([`Request::skills`] and its workflow step's plugin), read from
    /// the store and checked against their install.
    skills: Vec<(Skill, Grant)>,
    /// Every plugin the run names, as installed (ADR-0102): what its grants bound and what a
    /// deterministic step runs.
    plugins: BTreeMap<PluginName, (Skill, Grant)>,
    /// The store they were read from, which the broker reads a component from.
    plugin_store: PathBuf,
    /// The lock on `held_file` while the actor waits: `hyalos release` refuses a file nobody holds,
    /// so a run whose runner stopped is never released to.
    held_lock: Option<std::fs::File>,
}

#[cfg(test)]
mod tests {
    use super::step::broker_substrate;
    use super::*;
    use hyalos_plugin::skill::McpServer;
    use hyalos_types::Provenance;
    use hyalos_types::workflow::TypedValue;

    fn args(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_second_interrupt_message_has_no_runs_of_spaces() {
        let message = again_message("SIGINT");
        assert!(!message.contains("  "), "{message}");
        assert!(message.contains("GitHub token may still be live"));
        assert!(message.contains("expire within an hour"));
        assert!(message.starts_with("hyalos: interrupted again (SIGINT), "));
    }

    #[test]
    fn the_command_line_needs_a_repo_and_an_issue() {
        let request = parse(&args(&[
            "--repo",
            "octo/repo",
            "--issue",
            "fix it",
            "--harness",
            "claude-code",
            "--allow-host",
            "docs.rs",
            "--minutes",
            "5",
        ]))
        .unwrap();
        assert_eq!(request.repo, "octo/repo");
        assert_eq!(request.harness, Harness::ClaudeCode);
        assert_eq!(request.minutes, 5);
        assert_eq!(request.base, "main");
        assert!(request.egress_hosts.contains("docs.rs"));
        assert_eq!(
            step_model(&request),
            (
                Protocol::Anthropic,
                DEFAULT_ANTHROPIC_STEP_MODEL.to_string()
            )
        );
        // An issue number with the operator's description of the work, and another step model.
        let both = parse(&args(&[
            "--repo",
            "octo/repo",
            "--issue",
            "x",
            "--issue-number",
            "7",
            "--step-model",
            "small-1",
        ]))
        .unwrap();
        assert_eq!(both.issue_number, Some(7));
        assert_eq!(step_model(&both), (Protocol::OpenAi, "small-1".to_string()));
        let released = parse(&args(&[
            "--repo",
            "octo/repo",
            "--issue-number",
            "7",
            "--release-issue",
        ]))
        .unwrap();
        assert!(released.release_issue && !both.release_issue);
        for bad in [
            &["--issue", "x"][..],
            &["--repo", "octo", "--issue", "x"],
            &["--repo", "octo/../x", "--issue", "x"],
            &["--repo", "octo/repo"],
            &["--repo", "octo/repo", "--issue", " "],
            &["--repo", "octo/repo", "--issue", "x", "--harness", "vim"],
            &["--repo", "octo/repo", "--issue", "x", "--minutes", "0"],
            &["--repo", "octo/repo", "--issue", "x", "--surprise"],
            &["--repo", "octo/repo", "--issue-number", "0"],
            &["--repo", "octo/repo", "--issue-number", "seven"],
            &["--repo", "octo/repo", "--issue-number", "7", "--step-model"],
            &["--repo", "octo/repo", "--issue", "x", "--release-issue"],
            &[
                "--repo",
                "octo/repo",
                "--issue",
                "x",
                "--allow-host",
                "Docs.rs",
            ],
            &[
                "--repo",
                "octo/repo",
                "--issue",
                "x",
                "--allow-host",
                "docs.rs:443",
            ],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_reason_a_job_ends_with_is_a_distinct_code() {
        for text in why::ALL.iter().chain(&[
            workflow_run::codes::NOT_RUNNABLE,
            workflow_run::codes::INPUT_MISSING,
            workflow_run::codes::INPUTS_MISMATCH,
            "attestation.no_sandbox",
            "reader.bad_answer",
            "handle.operator_flag",
            "handle.broker_unavailable",
        ]) {
            assert_eq!(
                ReasonCode::new(text).as_ref().map(ReasonCode::as_str),
                Some(*text)
            );
            assert_eq!(code(text).as_str(), *text);
        }
        assert_eq!(
            why::ALL.iter().collect::<BTreeSet<_>>().len(),
            why::ALL.len()
        );
        assert_eq!(broker_substrate().as_str(), "broker");
    }

    #[test]
    fn the_prompt_goes_last_and_names_the_branch_and_the_helper() {
        let request = parse(&args(&["--repo", "octo/repo", "--issue", "fix it"])).unwrap();
        let planned = plan(&request).unwrap();
        assert!(matches!(planned, Planned::Actor));
        assert!(planned.result().is_none());
        let (prompt, inputs) =
            actor_prompt(&request, &planned, &Values::default(), None, "hyalos/job-4")
                .map_err(|f| f.why)
                .unwrap();
        assert_eq!(inputs, StepInputs::of([Provenance::Operator]));
        assert!(prompt.contains("The task:\n\nfix it\n"));
        assert!(prompt.contains("git clone http://127.0.0.1:8089/git/octo/repo.git"));
        assert!(prompt.contains("hyalos-pr octo/repo hyalos/job-4 main"));
        let codex = Harness::Codex.command(prompt.clone(), &[]);
        assert_eq!(codex[0], "hyalos-forward");
        assert_eq!(codex.last(), Some(&prompt));
        let claude = Harness::ClaudeCode.command(prompt.clone(), &[]);
        assert_eq!(
            &claude[..4],
            &["hyalos-forward", "claude", "-p", prompt.as_str()]
        );
    }

    /// A skill that is an MCP server is registered with either harness, from its read-only place in
    /// the sandbox and through the sandbox's proxy (ADR-0101); a skill that is not one adds nothing.
    #[test]
    fn a_skills_mcp_server_is_registered_with_the_harness() {
        let skill = |name: &str, mcp: Option<McpServer>| {
            let skill = Skill {
                name: PluginName::new(name).unwrap(),
                asks: hyalos_types::Capabilities::none(),
                mcp,
                files: BTreeMap::new(),
            };
            (skill, Grant::none())
        };
        let server = McpServer {
            command: "./bin/serve".into(),
            args: vec!["--quiet".into()],
        };
        let skills = [skill("notes", Some(server)), skill("pdf", None)];
        let codex = Harness::Codex.command("p".into(), &skills);
        let at = "/opt/hyalos/skills/notes/bin/serve";
        assert!(codex.contains(&format!("mcp_servers.notes.command=\"{at}\"")));
        assert!(codex.contains(&"mcp_servers.notes.args=[\"--quiet\"]".to_string()));
        let env = codex
            .iter()
            .find(|a| a.starts_with("mcp_servers.notes.env="))
            .unwrap();
        assert!(env.contains("HTTPS_PROXY = \"http://127.0.0.1:8089\""));
        assert!(!codex.iter().any(|a| a.contains("pdf")));
        assert_eq!(codex.last().map(String::as_str), Some("p"));
        let claude = Harness::ClaudeCode.command("p".into(), &skills);
        let config = claude.iter().position(|a| a == "--mcp-config").unwrap();
        let config: serde_json::Value = serde_json::from_str(&claude[config + 1]).unwrap();
        assert_eq!(config["mcpServers"]["notes"]["command"], at);
        assert_eq!(config["mcpServers"]["notes"]["args"][0], "--quiet");
        assert_eq!(
            config["mcpServers"]["notes"]["env"]["HTTPS_PROXY"],
            "http://127.0.0.1:8089"
        );
        assert!(config["mcpServers"].get("pdf").is_none());
        assert!(claude.contains(&"--strict-mcp-config".to_string()));
        assert!(claude.ends_with(&["--allowedTools".into(), "Bash".into(), "mcp__notes".into()]));
        // With no server, neither harness is given one.
        let plain = Harness::ClaudeCode.command("p".into(), &skills[1..]);
        assert!(plain.contains(&"--strict-mcp-config".to_string()));
        assert!(
            !plain
                .iter()
                .any(|a| a.starts_with("--mcp") || a.starts_with("mcp__"))
        );
        assert!(
            !Harness::Codex
                .command("p".into(), &skills[1..])
                .contains(&"-c".to_string())
        );
    }

    /// `--issue-number` runs the fix-issue workflow: the actor's prompt holds the operator's words and
    /// typed values, and is minted with the inputs the check derived for its step.
    #[test]
    fn an_issue_number_runs_the_workflow_and_the_actor_gets_typed_values() {
        let request = parse(&args(&["--repo", "octo/repo", "--issue-number", "7"])).unwrap();
        let planned = plan(&request).unwrap();
        let Planned::Workflow(flow, supplied) = &planned else {
            panic!("an issue number runs the workflow");
        };
        assert_eq!(
            supplied.task.as_deref(),
            Some("Resolve issue #7 of octo/repo.")
        );
        let mut values = Values::default();
        let [triage, plan_step] = flow.typed_steps() else {
            panic!("two typed steps");
        };
        assert!(flow.accept(
            triage,
            &[
                TypedValue::Bool(true),
                TypedValue::Bool(true),
                TypedValue::Choice(0),
                TypedValue::Choice(0)
            ],
            &mut values
        ));
        assert!(flow.accept(
            plan_step,
            &[TypedValue::Bool(true), TypedValue::Choice(1)],
            &mut values
        ));
        let (prompt, inputs) = actor_prompt(&request, &planned, &values, None, "hyalos/job-9")
            .map_err(|f| f.why)
            .unwrap();
        assert_eq!(
            inputs.reads,
            BTreeSet::from([Provenance::Operator, Provenance::ModelFromTainted])
        );
        assert!(
            prompt.contains("This work is for issue #7 of the repository."),
            "{prompt}"
        );
        assert!(
            prompt.contains("Resolve issue #7 of octo/repo."),
            "{prompt}"
        );
        assert!(prompt.contains("- triage.kind: bug\n"), "{prompt}");
        assert!(
            prompt.contains("- plan.approach: fix_with_tests\n"),
            "{prompt}"
        );
        assert!(prompt.contains("git push origin hyalos/job-9"), "{prompt}");
        // The actor returns its step's output through hyalos-submit, last (ADR-0064).
        assert_eq!(planned.result(), flow.actor().output.as_ref());
        assert!(
            prompt.contains("- outcome: one of \"pr_opened\", \"no_change\", \"blocked\"\n")
                && prompt.contains(
                    "For example: hyalos-submit '{\"outcome\":\"pr_opened\",\"tests_added\":true}'"
                ),
            "{prompt}"
        );
        assert!(
            prompt.find("hyalos-submit").unwrap() > prompt.find("hyalos-pr").unwrap(),
            "{prompt}"
        );
        assert!(!prompt.contains("<issue"), "{prompt}");
        assert!(!prompt.contains("hyalos-handle"), "{prompt}");

        // A released handle adds its digest and the helper, and nothing to what the prompt read.
        let handle = Digest::from_bytes([0xab; 32]);
        let (with, read) = actor_prompt(&request, &planned, &values, Some(&handle), "hyalos/job-9")
            .map_err(|f| f.why)
            .unwrap();
        assert!(
            with.contains(&format!("Read it with: hyalos-handle {handle}\n")),
            "{with}"
        );
        assert_eq!(read, inputs);
        assert!(
            with.find("hyalos-handle").unwrap() < with.find("git clone").unwrap(),
            "the engine's steps stay last"
        );
    }
}
