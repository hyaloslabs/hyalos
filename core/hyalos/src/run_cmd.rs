//! `hyalos run`: one cut-down reference job from the command line (S3.10, ADR-0050).
//!
//! The operator gives a repository and the text of an issue. The engine finds a ready substrate,
//! records the task and the job (with its substrate and credential tier), signs a manifest the
//! policy allows, mints a GitHub token for that one repository, and starts the broker and the
//! sandbox. The harness in the sandbox clones, pushes its branch and opens a pull request, all through
//! the job's socket; no credential enters the sandbox. While it runs, every gate answer the broker
//! reports, including egress refusals it sends unprompted, is recorded as `gate.answered`. When the
//! sandbox ends the engine ends the job in the broker, revokes the token, and records how it ended.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use hyalos_engine::{Engine, Ruling};
use hyalos_policy::{GateRequest, Outcome as GateOutcome};
use hyalos_substrate::{Choice, Ending, HOST_OS, SandboxSpec, Unsupported, detect};
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Answer, Budget, Clock, CredentialTier, Digest, Event, ForgeToken, HoldReason, JobId,
    JobOutcome, MANIFEST_VERSION, Manifest, Origin, Provenance, Role, Stakes, StepInputs,
    Timestamp,
};

use crate::clock::SystemClock;
use crate::doctor_cmd::{self, DEFAULT_IMAGE};
use crate::github::GitHubApp;
use crate::keys;
use crate::launch::{BrokerProcess, Incoming, JobDir, fresh_manifest_key};
use crate::paths::{self, Env};

const USAGE: &str = "usage: hyalos run --repo <owner/name> (--issue <text> | --issue-file <file>)
                  [--base <branch>] [--harness codex|claude-code] [--image <image>]
                  [--budget-tokens <n>] [--minutes <n>] [--allow-host <host>]...
  runs one job: a harness in a sandbox works on the issue in <owner/name>, pushes the branch
  hyalos/job-<id> and opens a pull request into --base (default main). It needs hyalos init, an
  approved policy that allows read, push and open_pr (hyalos policy approve), provider.keys and the
  GitHub App (github.app, github-app.pem) in the config directory, and a ready substrate (hyalos
  doctor). --allow-host lets the job reach a web host through the egress gate, if policy allows it.
  Defaults: harness codex, image hyalos-sandbox:latest, 200000 tokens, 30 minutes";

/// How long finding a substrate may take. A runtime that does not answer refuses the job rather than
/// hanging it (ADR-0043, ADR-0050).
pub const DETECT_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the broker may take to start and to answer the engine.
const BROKER_WAIT: Duration = Duration::from_secs(20);
/// How often the engine looks up from the broker's channel to see whether the sandbox has ended.
const POLL: Duration = Duration::from_millis(200);
/// Where a job reaches its repository, through the sandbox's forwarder (ADR-0046, ADR-0049).
const FORWARDER: &str = "http://127.0.0.1:8089";

/// What the operator asked for.
#[derive(Debug, Clone)]
pub struct Request {
    pub repo: String,
    pub issue: String,
    pub base: String,
    pub image: String,
    pub harness: Harness,
    pub budget_tokens: u64,
    pub minutes: u64,
    pub egress_hosts: BTreeSet<String>,
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
    fn command(&self, prompt: String) -> Vec<String> {
        let mut command: Vec<String> = match self {
            Harness::Codex => ["hyalos-forward", "codex", "exec", "--skip-git-repo-check"]
                .map(String::from)
                .to_vec(),
            Harness::ClaudeCode => {
                return [
                    "hyalos-forward",
                    "claude",
                    "-p",
                    &prompt,
                    "--tools",
                    "Bash",
                    "--allowedTools",
                    "Bash",
                ]
                .map(String::from)
                .to_vec();
            }
            Harness::Command(command) => command.clone(),
        };
        command.push(prompt);
        command
    }
}

/// Finds the substrates on a host. [`detect`] in the binary; tests pass another.
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
}

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
    let request = match parse(args) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("hyalos: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let env = |name: &str| std::env::var(name).ok();
    let setup = match setup(&env) {
        Ok(setup) => setup,
        Err(message) => {
            eprintln!("hyalos: {message}");
            return ExitCode::from(2);
        }
    };
    match run_job(&request, setup, &mut std::io::stdout()) {
        Ok(summary) if summary.ok() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(message) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(1)
        }
    }
}

fn setup(env: Env<'_>) -> Result<Setup, String> {
    Ok(Setup {
        config_dir: paths::config_dir(env)?,
        log: paths::log_file(env)?,
        runtime_dir: paths::runtime_dir(env)?,
        broker_exe: std::env::current_exe()
            .map_err(|e| format!("cannot find this executable: {e}"))?,
        detect: Box::new(detect),
        detect_timeout: DETECT_TIMEOUT,
    })
}

pub fn parse(args: &[String]) -> Result<Request, String> {
    let mut request = Request {
        repo: String::new(),
        issue: String::new(),
        base: "main".into(),
        image: DEFAULT_IMAGE.into(),
        harness: Harness::Codex,
        budget_tokens: 200_000,
        minutes: 30,
        egress_hosts: BTreeSet::new(),
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        let number = |text: String| {
            text.parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{flag} takes a positive number"))
        };
        match flag.as_str() {
            "--repo" => request.repo = value()?,
            "--issue" => request.issue = value()?,
            "--issue-file" => {
                let file = value()?;
                request.issue = std::fs::read_to_string(&file)
                    .map_err(|e| format!("cannot read {file}: {e}"))?;
            }
            "--base" => request.base = value()?,
            "--image" => request.image = value()?,
            "--harness" => {
                request.harness = match value()?.as_str() {
                    "codex" => Harness::Codex,
                    "claude-code" => Harness::ClaudeCode,
                    other => return Err(format!("no harness called {other}")),
                }
            }
            "--budget-tokens" => request.budget_tokens = number(value()?)?,
            "--minutes" => request.minutes = number(value()?)?,
            "--allow-host" => {
                request.egress_hosts.insert(value()?);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    let part_ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    match request.repo.split_once('/') {
        Some((owner, name)) if part_ok(owner) && part_ok(name) => {}
        _ => return Err("--repo is owner/name".into()),
    }
    if request.issue.trim().is_empty() {
        return Err("the issue text is empty; give --issue or --issue-file".into());
    }
    Ok(request)
}

/// What the harness is told. The text of the issue is the operator's, given on the command line.
fn prompt(request: &Request, branch: &str) -> String {
    let repo = &request.repo;
    let base = &request.base;
    format!(
        "You are working on the GitHub repository {repo}. The task:\n\n{issue}\n\n\
         Work like this, in the shell:\n\
         1. git clone {FORWARDER}/git/{repo}.git /tmp/work && cd /tmp/work\n\
         2. git checkout -b {branch}\n\
         3. Make the change and commit it.\n\
         4. git push origin {branch}\n\
         5. Write a pull request description to /tmp/pr.md, then run:\n   \
         hyalos-pr {repo} {branch} {base} \"<a short title>\" /tmp/pr.md\n\n\
         Only the branch {branch} can be pushed, and only into {base} can a pull request be opened. \
         There are no credentials in this sandbox and none are needed: the broker adds them.\n",
        issue = request.issue.trim_end(),
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

/// Runs one job and writes what happens to `out`. `Err` is a refusal before anything ran; once the
/// job is recorded, how it ended is in the [`Summary`] and in the log.
pub fn run_job(request: &Request, setup: Setup, out: &mut dyn Write) -> Result<Summary, String> {
    // 1. A ready substrate, or a refusal that says why, before anything is written.
    let Setup {
        config_dir,
        log,
        runtime_dir,
        broker_exe,
        detect,
        detect_timeout,
    } = setup;
    let substrate = choose(&request.image, detect, detect_timeout)?;
    let chosen_name = substrate.chosen_name().ok_or("no substrate was chosen")?;
    let driver = substrate.chosen.ok_or("no substrate was chosen")?;
    let _ = writeln!(out, "substrate: {chosen_name}");

    // 2. Everything else the job needs, still before anything is written.
    let app = GitHubApp::load(&config_dir).map_err(|e| e.to_string())?;
    let provider_keys = config_dir.join(paths::PROVIDER_KEYS_FILE);
    if !provider_keys.exists() {
        return Err(format!(
            "there is no {}: the harness could not call a model. See docs/operations/provider-keys.md",
            provider_keys.display()
        ));
    }
    let key = keys::load(&config_dir.join(paths::ENGINE_KEY_FILE)).map_err(|e| e.to_string())?;
    let mut engine = Engine::open(&log, key).map_err(|e| e.to_string())?;
    let mut clock = Stamps(SystemClock.now());

    // 3. The task and the job, with where it runs and where its credentials are.
    let digest = request_digest(request);
    let task = engine
        .submit(
            clock.now(),
            Origin::Engine,
            Event::TaskCreated { request: digest },
        )
        .map_err(|e| e.to_string())?;
    let started = engine
        .submit(
            clock.now(),
            Origin::Engine,
            Event::JobStarted {
                task: hyalos_types::TaskId::from_seq(task.seq),
                substrate: chosen_name,
                credentials: CredentialTier::BrokerHeld,
            },
        )
        .map_err(|e| e.to_string())?;
    let job = JobId::from_seq(started.seq);
    let _ = writeln!(out, "job {}: started (task {})", job.seq(), task.seq);
    let mut ended = Ended {
        engine: &mut engine,
        clock: &mut clock,
        job,
    };

    // 4. The manifest, within policy; then the token, for that one repository.
    let branch = format!("hyalos/job-{}", job.seq());
    let deadline = Duration::from_secs(request.minutes.saturating_mul(60));
    let now = ended.clock.now();
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        job,
        role: Role::Actor,
        tainted: true,
        repo: request.repo.clone(),
        branch_prefix: branch.clone(),
        pr_base: Some(request.base.clone()),
        actions: ["read", "push", "open_pr"].map(String::from).into(),
        egress_hosts: request.egress_hosts.clone(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget {
            tokens: request.budget_tokens,
        },
        deadline: now.saturating_add(deadline),
        approval: None,
    };
    let manifest_key = match fresh_manifest_key() {
        Ok(key) => key,
        Err(e) => return ended.failed(out, &format!("no manifest key: {e}")),
    };
    // The issue text is the operator's; an actor runs tainted whatever it reads (ADR-0005).
    let inputs = StepInputs::of([Provenance::Operator]);
    let signed = match ended
        .engine
        .mint_manifest(&manifest, &inputs, &manifest_key, now)
    {
        Ok(signed) => signed,
        Err(e) => return ended.failed(out, &format!("no manifest: {e}")),
    };
    let token = match app.mint(&manifest, now) {
        Ok(Some(minted)) => minted.token,
        Ok(None) => return ended.failed(out, "no forge token: the job asks for no forge action"),
        Err(e) => return ended.failed(out, &format!("no forge token: {e}")),
    };
    let _ = writeln!(out, "job {}: token minted for {}", job.seq(), request.repo);

    // 5. The broker and the job's socket. From here every way out revokes the token.
    let revoke = |out: &mut dyn Write| -> Result<(), String> {
        let revoked = app.revoke(&token).map_err(|e| e.to_string());
        let _ = match &revoked {
            Ok(()) => writeln!(out, "job {}: forge token revoked", job.seq()),
            Err(e) => writeln!(
                out,
                "job {}: REVOKING THE FORGE TOKEN FAILED: {e}. Revoke it on GitHub (the App's \
                 installation tokens expire within an hour); see docs/operations/github-app.md",
                job.seq()
            ),
        };
        revoked
    };
    let prepared = start_broker(
        &runtime_dir,
        &broker_exe,
        &provider_keys,
        &manifest_key.public(),
        &app,
        job,
        &signed.to_bytes().map_err(|e| e.to_string()),
        &token,
    );
    let (mut broker, job_dir) = match prepared {
        Ok(ready) => ready,
        Err(e) => {
            let revoked = revoke(out);
            return ended.with(out, JobOutcome::Failed, &e, revoked);
        }
    };
    let _ = writeln!(out, "job {}: admitted by the broker", job.seq());

    // 6. The sandbox, and the broker's reports while it runs.
    let mut spec = SandboxSpec::new(
        job,
        &request.image,
        request.harness.command(prompt(request, &branch)),
        job_dir.socket(),
    );
    spec.deadline = deadline;
    spec.env = BTreeMap::from([
        ("HYALOS_REPO".into(), request.repo.clone()),
        ("HYALOS_BRANCH".into(), branch.clone()),
        ("HYALOS_BASE".into(), request.base.clone()),
        (
            "HYALOS_REMOTE".into(),
            format!("{FORWARDER}/git/{}.git", request.repo),
        ),
    ]);
    let mut sandbox = match driver.start(&spec) {
        Ok(sandbox) => sandbox,
        Err(e) => {
            let _ = broker.send(&ToBroker::EndJob { job });
            let revoked = revoke(out);
            return ended.with(
                out,
                JobOutcome::Failed,
                &format!("the sandbox did not start: {e}"),
                revoked,
            );
        }
    };
    let _ = writeln!(out, "job {}: sandbox {} running", job.seq(), sandbox.id());
    let (done_tx, done) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done_tx.send(sandbox.wait());
    });
    let mut state = Reported::default();
    let outcome = loop {
        if let Ok(outcome) = done.try_recv() {
            break outcome;
        }
        if state.broker_gone {
            // Nothing more can be reported; the job's socket is gone, so the sandbox ends soon.
            match done.recv() {
                Ok(outcome) => break outcome,
                Err(_) => return Err("the sandbox's waiter stopped".into()),
            }
        }
        state.take(&mut broker, POLL, &mut ended, out);
    };
    // What the broker reported just before the sandbox ended.
    while !state.broker_gone && state.take(&mut broker, POLL, &mut ended, out) {}
    let _ = writeln!(
        out,
        "job {}: sandbox ended ({:?})\n--- harness output ---\n{}{}--- end ---",
        job.seq(),
        outcome.ending,
        outcome.stdout_text(),
        String::from_utf8_lossy(&outcome.stderr)
    );

    // 7. End the job in the broker, revoke the token, and record how it ended.
    if !state.broker_gone {
        let _ = broker.send(&ToBroker::EndJob { job });
        let _ = broker.send(&ToBroker::Shutdown);
    }
    drop(broker);
    drop(job_dir);
    let revoked = revoke(out);
    if state.held {
        let _ = writeln!(out, "job {}: held; see the open hold in the log", job.seq());
        return Ok(Summary {
            job,
            outcome: JobOutcome::Held,
            revoked,
        });
    }
    let (result, why) = match (&state.refused, outcome.ending) {
        (Some(reason), _) => (JobOutcome::Failed, format!("the broker refused ({reason})")),
        (None, _) if state.broker_gone => (JobOutcome::Failed, "the broker stopped".into()),
        (None, Ending::Exited(0)) => (JobOutcome::Succeeded, "the harness finished".into()),
        (None, Ending::DeadlineExceeded) => (
            JobOutcome::DeadlineExceeded,
            "the deadline passed".to_string(),
        ),
        (None, ending) => (JobOutcome::Failed, format!("the sandbox ended {ending:?}")),
    };
    ended.with(out, result, &why, revoked)
}

/// Runs `detect` with a timeout, and refuses a host that cannot run a job with what the operator
/// needs to know.
fn choose(image: &str, detect: Detect, timeout: Duration) -> Result<Choice, String> {
    let (tx, rx) = mpsc::channel();
    let image_owned = image.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(detect(&image_owned));
    });
    let detected = match rx.recv_timeout(timeout) {
        Ok(detected) => detected,
        Err(_) => {
            return Err(format!(
                "finding a substrate took longer than {} s, so a container runtime is not answering \
                 and no job was started. Check that `podman info` or `docker info` returns, then run \
                 hyalos doctor",
                timeout.as_secs()
            ));
        }
    };
    match detected {
        Err(unsupported) => Err(unsupported.to_string()),
        Ok(choice) if choice.chosen.is_none() => {
            let checked = Ok(choice);
            let (report, _) = doctor_cmd::report(HOST_OS, &checked, image);
            Err(format!(
                "no substrate is ready, so no job was started. hyalos doctor says:\n{report}"
            ))
        }
        Ok(choice) => Ok(choice),
    }
}

#[allow(clippy::too_many_arguments)]
fn start_broker(
    runtime_dir: &Path,
    broker_exe: &Path,
    provider_keys: &Path,
    manifest_public: &hyalos_types::ManifestPublicKey,
    app: &GitHubApp,
    job: JobId,
    manifest: &Result<Vec<u8>, String>,
    token: &ForgeToken,
) -> Result<(BrokerProcess, JobDir), String> {
    let manifest = manifest.clone()?;
    paths::make_private_dir(runtime_dir)?;
    let mut broker = BrokerProcess::launch_configured(
        broker_exe,
        runtime_dir,
        manifest_public,
        Some(provider_keys),
        Some(app.forge_bases()),
        BROKER_WAIT,
    )
    .map_err(|e| format!("the broker did not start: {e}"))?;
    let job_dir = JobDir::create(runtime_dir).map_err(|e| format!("no job directory: {e}"))?;
    broker
        .send(&ToBroker::StartJob {
            job,
            role: Role::Actor,
            manifest,
            socket: job_dir.socket().to_string_lossy().into_owned(),
            forge_token: Some(token.clone()),
        })
        .map_err(|e| format!("the broker did not take the job: {e}"))?;
    match broker.recv() {
        Ok(Some(ToEngine::Admitted { job: admitted })) if admitted == job => Ok((broker, job_dir)),
        Ok(Some(ToEngine::Refused { reason, .. })) => {
            Err(format!("the broker refused the job ({reason})"))
        }
        Ok(other) => Err(format!("the broker answered {other:?}")),
        Err(e) => Err(format!("the broker did not answer: {e}")),
    }
}

/// The digest a task records of what was asked for.
fn request_digest(request: &Request) -> Digest {
    let text = serde_json::json!({
        "repo": request.repo,
        "base": request.base,
        "issue": request.issue,
    })
    .to_string();
    let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes());
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(hash.as_ref());
    Digest::from_bytes(bytes)
}

/// Writes the job's last events.
struct Ended<'a> {
    engine: &'a mut Engine,
    clock: &'a mut Stamps,
    job: JobId,
}

impl Ended<'_> {
    /// Ends the job failed before a token existed.
    fn failed(&mut self, out: &mut dyn Write, why: &str) -> Result<Summary, String> {
        self.with(out, JobOutcome::Failed, why, Ok(()))
    }

    fn with(
        &mut self,
        out: &mut dyn Write,
        outcome: JobOutcome,
        why: &str,
        revoked: Result<(), String>,
    ) -> Result<Summary, String> {
        let job = self.job;
        let recorded = self.engine.submit(
            self.clock.now(),
            Origin::Engine,
            Event::JobEnded { job, outcome },
        );
        match &recorded {
            Ok(head) => writeln!(
                out,
                "job {}: {outcome:?}: {why} (event {})",
                job.seq(),
                head.seq
            ),
            Err(e) => writeln!(
                out,
                "job {}: {outcome:?}: {why}; recording that failed: {e}",
                job.seq()
            ),
        }
        .map_err(|e| e.to_string())?;
        recorded.map_err(|e| e.to_string())?;
        Ok(Summary {
            job,
            outcome,
            revoked,
        })
    }
}

/// What the broker has said about the job.
#[derive(Default)]
struct Reported {
    refused: Option<String>,
    held: bool,
    broker_gone: bool,
}

impl Reported {
    /// Handles the broker's next message, if one comes within `wait`. False if none did.
    fn take(
        &mut self,
        broker: &mut BrokerProcess,
        wait: Duration,
        ended: &mut Ended<'_>,
        out: &mut dyn Write,
    ) -> bool {
        let job = ended.job;
        let message = match broker.poll(wait) {
            Ok(Incoming::Message(message)) => message,
            Ok(Incoming::Nothing) => return false,
            Ok(Incoming::Closed) | Err(_) => {
                let _ = writeln!(out, "job {}: the broker closed its channel", job.seq());
                self.broker_gone = true;
                return false;
            }
        };
        match message {
            ToEngine::Gate {
                job: reported,
                gate,
                inputs,
                answer,
                reason,
            } if reported == job => {
                let at = ended.clock.now();
                let recorded = if answer == Answer::Unknown {
                    // Never sent by today's broker; if it is, the job holds as ADR-0020 says, and
                    // only a human can release it.
                    let request = GateRequest {
                        gate,
                        inputs,
                        stakes: Stakes::irreversible(),
                    };
                    let outcome = GateOutcome::Hold {
                        reason: reason.clone(),
                        why: HoldReason::Unknown,
                    };
                    ended
                        .engine
                        .rule_gate(at, Origin::Broker, job, &request, outcome)
                        .map(|ruling| {
                            self.held = matches!(ruling, Ruling::Held(_));
                            ended
                                .engine
                                .head()
                                .ok()
                                .flatten()
                                .map(|h| h.seq)
                                .unwrap_or_default()
                        })
                } else {
                    ended
                        .engine
                        .submit(
                            at,
                            Origin::Broker,
                            Event::GateAnswered {
                                job,
                                gate,
                                inputs,
                                answer,
                                reason: reason.clone(),
                            },
                        )
                        .map(|head| head.seq)
                };
                let _ = match recorded {
                    Ok(seq) => writeln!(
                        out,
                        "job {}: gate {gate:?} answered {answer:?} ({reason}), recorded as event {seq}",
                        job.seq()
                    ),
                    Err(e) => writeln!(
                        out,
                        "job {}: gate {gate:?} answered {answer:?} ({reason}), not recorded: {e}",
                        job.seq()
                    ),
                };
            }
            ToEngine::Refused {
                job: reported,
                reason,
            } if reported == job => {
                let _ = writeln!(
                    out,
                    "job {}: the broker ended the job ({reason})",
                    job.seq()
                );
                self.refused = Some(reason.to_string());
            }
            other => {
                let _ = writeln!(out, "job {}: ignored from the broker: {other:?}", job.seq());
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
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
        for bad in [
            &["--issue", "x"][..],
            &["--repo", "octo", "--issue", "x"],
            &["--repo", "octo/../x", "--issue", "x"],
            &["--repo", "octo/repo"],
            &["--repo", "octo/repo", "--issue", " "],
            &["--repo", "octo/repo", "--issue", "x", "--harness", "vim"],
            &["--repo", "octo/repo", "--issue", "x", "--minutes", "0"],
            &["--repo", "octo/repo", "--issue", "x", "--surprise"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_prompt_goes_last_and_names_the_branch_and_the_helper() {
        let request = parse(&args(&["--repo", "octo/repo", "--issue", "fix it"])).unwrap();
        let prompt = prompt(&request, "hyalos/job-4");
        assert!(prompt.contains("git clone http://127.0.0.1:8089/git/octo/repo.git"));
        assert!(prompt.contains("hyalos-pr octo/repo hyalos/job-4 main"));
        let codex = Harness::Codex.command(prompt.clone());
        assert_eq!(codex[0], "hyalos-forward");
        assert_eq!(codex.last(), Some(&prompt));
        let claude = Harness::ClaudeCode.command(prompt.clone());
        assert_eq!(
            &claude[..4],
            &["hyalos-forward", "claude", "-p", prompt.as_str()]
        );
    }
}
