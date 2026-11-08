//! Runs the MCP server starts for the driving agent, and their files (S5.2, ADR-0069).
//!
//! The server never opens `engine.key`. It writes the run's request to `runs/` in the data directory
//! and starts `hyalos run --mcp-start <nonce>`, a process of its own that holds the key, writes the
//! log and outlives the server. That runner locks the request's lock file, records `task.created`,
//! renames its files after the task (`task-<id>.json`, `.lock`, `.log`) and only then prints
//! `task <id>`, so the task is in the log before the server answers with it. The lock is held for the
//! whole run: a task whose request is here, which the log shows unfinished and whose lock no one
//! holds has lost its runner, and `hyalos run --resume <id>` carries it on from the log. The server
//! starts that itself when it starts and whenever it is asked about such a task.
//!
//! What a run's task looks like to the agent ([`view`]) is read from the log alone, with the engine's
//! public key, and the request document is trusted only if its digest is the one `task.created`
//! recorded.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use hyalos_engine::{State, TaskStatus};
use hyalos_types::provenance::Provenance;
use hyalos_types::workflow::{Field, TypedValue};
use hyalos_types::{JobOutcome, TaskId, Timestamp};
use serde_json::{Map, Value, json};

use crate::paths::{self, Env};
use crate::run_cmd::{self, Request, Stored, request_digest, why};
use crate::workflow_run;

/// The directory under the data directory that holds the runs' files.
pub const RUNS_DIR: &str = "runs";

/// How long the server waits for a runner to record its task: finding a substrate may take
/// [`run_cmd::DETECT_TIMEOUT`], and opening the log and the App a little more.
pub const START_WAIT: Duration = Duration::from_secs(90);

/// The files of the runs, in one owner-only directory.
#[derive(Debug, Clone)]
pub struct Runs {
    dir: PathBuf,
}

impl Runs {
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn from_env(env: Env<'_>) -> Result<Self, String> {
        Ok(Self::at(paths::data_dir(env)?.join(RUNS_DIR)))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `task-<id>.<ext>`: `json` the request, `lock` held by its runner, `log` what the runner
    /// printed, `cancel` the driving agent's cancel, `failed` a runner that stopped with an error,
    /// `held` the issue a runner holds for the operator to release (ADR-0078).
    pub fn file(&self, task: TaskId, ext: &str) -> PathBuf {
        self.dir.join(format!("task-{}.{ext}", task.seq()))
    }

    fn starting(&self, nonce: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("start-{nonce}.{ext}"))
    }

    /// The request `task` was started with, if the server started it and the document is the one
    /// the log recorded.
    pub fn request(&self, state: &State, task: TaskId) -> Option<Request> {
        let recorded = state.tasks().get(&task)?.request;
        let request = read_request(&self.file(task, "json")).ok()?;
        (request_digest(&request) == recorded).then_some(request)
    }

    /// The handle `task`'s runner holds for the operator to release, with the issue's repository and
    /// number, as the runner wrote them (ADR-0078), and only while that runner waits: it locks the
    /// file for as long as it does. Anything running as this user could write the file, so
    /// `hyalos release` reads the issue again and refuses unless its digest is this handle.
    pub fn held_handle(&self, task: TaskId) -> Result<HeldIssue, String> {
        let file = self.file(task, "held");
        let none = || format!("task {} has no run waiting for your release", task.seq());
        let opened = File::open(&file).map_err(|_| none())?;
        if opened.try_lock_shared().is_ok() {
            return Err(format!("{}: its runner stopped", none()));
        }
        let text = std::fs::read_to_string(&file).map_err(|_| none())?;
        let held: Value = serde_json::from_str(&text)
            .map_err(|_| format!("{} is not readable", file.display()))?;
        let handle = held["handle"]
            .as_str()
            .and_then(hyalos_types::Digest::parse)
            .ok_or_else(|| format!("{} names no handle", file.display()))?;
        let issue = held["issue"]
            .as_u64()
            .ok_or_else(|| format!("{} names no issue", file.display()))?;
        let repo = held["repo"]
            .as_str()
            .filter(|r| run_cmd::is_repo(r))
            .ok_or_else(|| format!("{} names no repository", file.display()))?
            .to_string();
        Ok(HeldIssue {
            handle,
            repo,
            issue,
        })
    }

    /// Whether `task`'s last runner stopped with an error, and nothing has retried it since.
    pub fn has_failed(&self, task: TaskId) -> bool {
        self.file(task, "failed").exists()
    }

    /// The runs a runner holds now. Compaction and pruning refuse while there are any, since a runner's
    /// engine reads the log from where it last looked (ADR-0069).
    pub fn held(&self) -> Vec<u64> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut held: Vec<u64> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|name| {
                name.strip_prefix("task-")?
                    .strip_suffix(".lock")?
                    .parse()
                    .ok()
            })
            .filter(|seq| self.is_running(TaskId::from_seq(*seq)))
            .collect();
        held.sort_unstable();
        held
    }

    /// Refuses while any run is held, naming them.
    pub fn none_running(&self) -> Result<(), String> {
        match self.held().as_slice() {
            [] => Ok(()),
            held => Err(format!(
                "a run is in progress (task {}); wait for it to end, or stop its runner, first",
                held.iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(", task ")
            )),
        }
    }

    /// Whether a runner holds `task`'s lock. A lock file that cannot be opened counts as held, so the
    /// server never starts a second runner on a guess.
    pub fn is_running(&self, task: TaskId) -> bool {
        match File::open(self.file(task, "lock")) {
            Ok(file) => file.try_lock().is_err(),
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        }
    }
}

/// The issue a waiting run holds, as its `.held` file names it (ADR-0078).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldIssue {
    pub handle: hyalos_types::Digest,
    pub repo: String,
    pub issue: u64,
}

/// Records that `task`'s runner stopped with an error, before it lets go of the lock, so the server
/// never starts it again by itself: the same error would stop each one, and a step that was running
/// would run, and spend, again on every poll. `hyalos run --resume` by hand clears it.
fn mark_failed(runs: &Runs, task: TaskId, message: &str) {
    let _ = std::fs::write(runs.file(task, "failed"), format!("{message}\n"));
}

fn read_request(path: &Path) -> Result<Request, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let stored: Stored = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{} is not a run document: {e}", path.display()))?;
    Request::from_stored(stored)
}

/// A new owner-only file, which must not exist.
fn create_private(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Opens (creating it if need be) and locks `path`. `Ok(None)` if another process holds it.
fn lock(path: &Path) -> Result<Option<File>, String> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("cannot lock {}: {e}", path.display())),
    }
}

/// `hyalos run --mcp-start <nonce>`: the run the server wrote `start-<nonce>.json` for. Its stdout
/// is the server's pipe, which gets the one line `task <id>`; its stderr is the run's log.
pub fn start(nonce: &str) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        eprintln!("hyalos: --mcp-start takes the server's 32 hexadecimal characters");
        return ExitCode::from(2);
    }
    let started = (|| {
        let runs = Runs::from_env(&env)?;
        let held = lock(&runs.starting(nonce, "lock"))?.ok_or("this run is already starting")?;
        let request = read_request(&runs.starting(nonce, "json"))?;
        let mut setup = run_cmd::setup(&env)?;
        let named = runs.clone();
        let nonce = nonce.to_string();
        let recorded = Arc::new(Mutex::new(None));
        let heard = recorded.clone();
        setup.on_task = Some(Box::new(move |task| {
            if let Ok(mut heard) = heard.lock() {
                *heard = Some(task);
            }
            // The lock first, so the request never stands under its task's name unlocked.
            for ext in ["lock", "log", "json"] {
                std::fs::rename(named.starting(&nonce, ext), named.file(task, ext))
                    .map_err(|e| format!("cannot name the run's files after task {task}: {e}"))?;
            }
            let mut stdout = std::io::stdout();
            writeln!(stdout, "task {}", task.seq())
                .and_then(|()| stdout.flush())
                .map_err(|e| format!("cannot tell the server task {task} started: {e}"))?;
            Ok(Some(named.file(task, "cancel")))
        }));
        let summary = run_cmd::run_job(&request, setup, &mut std::io::stderr());
        if let (Err(message), Ok(Some(task))) = (&summary, recorded.lock().map(|t| *t)) {
            mark_failed(&runs, task, message);
        }
        drop(held);
        Ok::<_, String>(summary?.ok())
    })();
    match started {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(1)
        }
    }
}

/// `hyalos run --resume <task> [--auto]`: carries on a run whose runner stopped, from the log. Exits 0
/// at once if another runner holds it. `--auto` is the server's: it leaves alone a run whose last
/// runner stopped with an error, which only a resume by hand retries.
pub fn resume(task: &str, auto: bool) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let Some(task) = task
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .map(TaskId::from_seq)
    else {
        eprintln!("hyalos: --resume takes a task's id");
        return ExitCode::from(2);
    };
    let resumed = (|| {
        let runs = Runs::from_env(&env)?;
        let Some(held) = lock(&runs.file(task, "lock"))? else {
            println!("task {task} is being run already");
            return Ok(true);
        };
        if auto && runs.has_failed(task) {
            println!("task {task} waits for the operator: its last runner stopped with an error");
            return Ok(true);
        }
        // Asked again by hand: the error a runner stopped with is this one's to meet.
        let _ = std::fs::remove_file(runs.file(task, "failed"));
        let summary = (|| {
            let request = read_request(&runs.file(task, "json"))?;
            let mut setup = run_cmd::setup(&env)?;
            setup.interrupt.watch(runs.file(task, "cancel"));
            run_cmd::resume_job(&request, task, setup, &mut std::io::stdout())
        })();
        if let Err(message) = &summary {
            mark_failed(&runs, task, message);
        }
        drop(held);
        Ok::<_, String>(summary?.is_none_or(|s| s.ok()))
    })();
    match resumed {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(1)
        }
    }
}

/// Starts runners: the server's side (ADR-0069).
#[derive(Debug, Clone)]
pub struct Launcher {
    pub runs: Runs,
    /// The `hyalos` executable a runner is started from.
    pub exe: PathBuf,
    /// How long a new runner may take to record its task.
    pub wait: Duration,
}

impl Launcher {
    /// A runner for `request`, detached from the server, and the task it recorded. `Err` says why
    /// nothing was started, in the runner's own words; then nothing is in the log.
    ///
    /// The nonce is set on `request` too, so its digest is this call's alone and [`Launcher::adopt`]
    /// with the same request can only find the task this call's runner recorded.
    pub fn start(&self, request: &mut Request) -> Result<TaskId, String> {
        paths::make_private_dir(&self.runs.dir)?;
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| format!("the system gave no randomness: {e}"))?;
        let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        request.nonce = Some(nonce.clone());
        let document = self.runs.starting(&nonce, "json");
        let log = self.runs.starting(&nonce, "log");
        let failed = |e: std::io::Error| format!("cannot write the run's request: {e}");
        let bytes = serde_json::to_vec(&request.stored()).map_err(|e| e.to_string())?;
        create_private(&document)
            .and_then(|mut f| f.write_all(&bytes))
            .map_err(failed)?;
        let stderr = create_private(&log).map_err(failed)?;
        let forget = || {
            for ext in ["json", "log", "lock"] {
                let _ = std::fs::remove_file(self.runs.starting(&nonce, ext));
            }
        };
        let child = Command::new(&self.exe)
            .args(["run", "--mcp-start", &nonce])
            .env_remove(crate::mcp_cmd::CREDENTIAL_VAR)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(stderr)
            // Out of the server's process group, so a Ctrl-C meant for the client does not reach it.
            .process_group(0)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                forget();
                return Err(format!("cannot start a runner: {e}"));
            }
        };
        let (tx, rx) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = BufReader::new(stdout).read_line(&mut line);
                let _ = tx.send(line);
            });
        }
        let said = rx.recv_timeout(self.wait).unwrap_or_default();
        let task = said
            .trim()
            .strip_prefix("task ")
            .and_then(|n| n.parse::<u64>().ok())
            .map(TaskId::from_seq);
        match task {
            Some(task) => {
                // Reaped on a thread, so it never waits for the run and leaves no zombie.
                std::thread::spawn(move || child.wait());
                Ok(task)
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let text = std::fs::read_to_string(&log).unwrap_or_default();
                forget();
                let why = text
                    .lines()
                    .rev()
                    .find_map(|l| l.strip_prefix("hyalos: "))
                    .unwrap_or("the runner stopped before it recorded a task");
                Err(why.to_string())
            }
        }
    }

    /// The task a runner that was stopped before it answered may still have recorded for `request`
    /// (as [`Launcher::start`] left it, with its nonce): the open one with its digest, created since
    /// `since`. The nonce makes the digest this call's alone, so no other run's task matches. Its request document is written if
    /// the runner had not named it yet, so the run resumes like any other (ADR-0069).
    pub fn adopt(&self, state: &State, request: &Request, since: Timestamp) -> Option<TaskId> {
        let digest = request_digest(request);
        let (task, _) = state.tasks().iter().rev().find(|(_, t)| {
            t.request == digest && t.created_at >= since && t.status() == TaskStatus::Open
        })?;
        let bytes = serde_json::to_vec(&request.stored()).ok()?;
        match create_private(&self.runs.file(*task, "json")) {
            Ok(mut file) => file.write_all(&bytes).ok()?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
        Some(*task)
    }

    /// Starts `hyalos run --resume <task>` if `task` is one of these runs, unfinished in `state`, and
    /// no runner holds it. Its output is added to the run's log.
    pub fn resume_if_stopped(&self, state: &State, task: TaskId) {
        let unfinished = state
            .tasks()
            .get(&task)
            .is_some_and(|t| t.status() == TaskStatus::Open);
        if !unfinished
            || last_job_held(state, task)
            || self.runs.request(state, task).is_none()
            || self.runs.has_failed(task)
            || self.runs.is_running(task)
        {
            return;
        }
        let Ok(log) = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(self.runs.file(task, "log"))
        else {
            return;
        };
        let Ok(err) = log.try_clone() else { return };
        let spawned = Command::new(&self.exe)
            .args(["run", "--resume", &task.seq().to_string(), "--auto"])
            .env_remove(crate::mcp_cmd::CREDENTIAL_VAR)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err)
            .process_group(0)
            .spawn();
        if let Ok(mut child) = spawned {
            std::thread::spawn(move || child.wait());
        }
    }

    /// Every unfinished run whose runner stopped, started again: what the server does when it starts.
    pub fn resume_all(&self, state: &State) {
        for task in state.tasks().keys() {
            self.resume_if_stopped(state, *task);
        }
    }

    /// Asks `task`'s runner to stop. Asking again, or after it ended, changes nothing.
    pub fn cancel(&self, state: &State, task: TaskId) -> Result<(), String> {
        let unfinished = state
            .tasks()
            .get(&task)
            .is_some_and(|t| t.status() == TaskStatus::Open);
        if unfinished {
            match create_private(&self.runs.file(task, "cancel")) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("cannot ask task {task} to stop: {e}")),
            }
            // A run with no runner is ended by one that finds the cancel at once.
            self.resume_if_stopped(state, task);
        }
        Ok(())
    }
}

/// Whether `task`'s last job ended held. Its run stopped there, and only a decision on the hold, never
/// a resume, carries it on (ADR-0029), though the task reads as open again once the hold is released.
fn last_job_held(state: &State, task: TaskId) -> bool {
    state
        .jobs()
        .values()
        .rfind(|job| job.task == task)
        .is_some_and(|job| {
            job.ended
                .is_some_and(|(_, outcome)| outcome == JobOutcome::Held)
        })
}

/// Where a run stands, as the agent is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// `done` of `of` steps have returned their values. `stopped`: no runner holds it; `failed`: its
    /// last runner stopped with an error, so only the operator starts another.
    Working {
        done: usize,
        of: usize,
        stopped: bool,
        failed: bool,
        /// The issue the actor's job waits for the operator to release (ADR-0078): the request
        /// asked for it, the job is running and the log has no release for the task yet.
        awaiting_release: Option<u64>,
    },
    /// Every step finished: the actor's result, field by field, as the workflow names its options,
    /// and where the values came from: a model that read tainted input unless the workflow shows
    /// the actor's step untainted (ADR-0005, ADR-0010).
    Succeeded {
        result: Map<String, Value>,
        provenance: Provenance,
    },
    /// The run is over without a result: how its last job ended and why.
    Ended { outcome: JobOutcome, reason: String },
    /// The driving agent cancelled it, or the operator did while it was held.
    Cancelled,
}

/// A run's task, read from the log.
#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub task: TaskId,
    pub created: Timestamp,
    /// Its last event.
    pub updated: Timestamp,
    pub phase: Phase,
}

/// `task` as the agent sees it, if the server started it and its request is the one recorded.
pub fn view(state: &State, runs: &Runs, task: TaskId) -> Option<RunView> {
    let request = runs.request(state, task)?;
    let recorded = state.tasks().get(&task)?;
    let flow = workflow_run::fix_issue().ok()?;
    let actor = flow
        .workflow()
        .steps
        .iter()
        .rfind(|s| s.role == hyalos_types::Role::Actor)?;
    let of = flow.workflow().steps.len();
    let jobs: Vec<_> = state
        .jobs()
        .values()
        .filter(|job| job.task == task)
        .collect();
    let mut updated = recorded.created_at;
    let mut done = 0;
    for job in &jobs {
        updated = updated.max(job.started_at);
        if let Some((at, outcome)) = job.ended {
            updated = updated.max(at);
            if outcome == JobOutcome::Succeeded && job.values.is_some() {
                done += 1;
            }
        }
    }
    if let Some(at) = recorded.completed_at {
        updated = updated.max(at);
    }
    let last = jobs.last();
    let reason = last
        .and_then(|j| j.reason.as_ref())
        .map(|r| r.as_str().to_string());
    let held = last_job_held(state, task);
    let phase = if let Some(cancelled) = &recorded.cancelled {
        updated = updated.max(cancelled.at);
        Phase::Cancelled
    } else if recorded.completed_at.is_none() && !held {
        Phase::Working {
            done,
            of,
            stopped: false,
            failed: false,
            awaiting_release: awaiting_release(state, task, &request),
        }
    } else if reason.as_deref() == Some(why::CANCELLED) {
        Phase::Cancelled
    } else if done == of
        && let Some(values) = last.and_then(|j| j.values.as_ref())
        && let Some(output) = &actor.output
    {
        // Unknown is tainted: only a step the workflow shows untainted is the operator's.
        let provenance = match flow.tainted(&actor.id) {
            Some(false) => Provenance::Operator,
            _ => Provenance::ModelFromTainted,
        };
        Phase::Succeeded {
            result: result(output, values),
            provenance,
        }
    } else {
        match last.and_then(|j| j.ended) {
            Some((_, outcome)) => Phase::Ended {
                outcome,
                reason: reason.unwrap_or_else(|| "legacy.unrecorded".into()),
            },
            None => Phase::Ended {
                outcome: JobOutcome::Failed,
                reason: "run.incomplete".into(),
            },
        }
    };
    let phase = match phase {
        Phase::Working {
            done,
            of,
            awaiting_release,
            ..
        } => Phase::Working {
            done,
            of,
            stopped: !runs.is_running(task),
            failed: runs.has_failed(task),
            awaiting_release,
        },
        other => other,
    };
    Some(RunView {
        task,
        created: recorded.created_at,
        updated,
        phase,
    })
}

/// The issue `task`'s actor waits for the operator to release, read from the log and the request
/// alone (ADR-0078): the request asked to wait, every step before the actor returned its values, the
/// actor's job is running, and the log has no release for the task. Only the actor's job waits, before
/// it mints anything, and any release to the task ends the wait.
pub fn awaiting_release(state: &State, task: TaskId, request: &Request) -> Option<u64> {
    if !request.await_release {
        return None;
    }
    let recorded = state.tasks().get(&task)?;
    let of = workflow_run::fix_issue().ok()?.workflow().steps.len();
    let jobs: Vec<_> = state.jobs().values().filter(|j| j.task == task).collect();
    let answered = jobs
        .iter()
        .filter(|j| matches!(j.ended, Some((_, JobOutcome::Succeeded))) && j.values.is_some())
        .count();
    let waiting = recorded.completed_at.is_none()
        && recorded.cancelled.is_none()
        && answered + 1 == of
        && jobs.last().is_some_and(|j| j.ended.is_none())
        && !state.handles().iter().any(|h| h.task == task);
    request.issue_number.filter(|_| waiting)
}

/// The actor's values by field: a truth value, a number, or the option's own text from the workflow.
fn result(output: &hyalos_types::workflow::Output, values: &[TypedValue]) -> Map<String, Value> {
    output
        .iter()
        .zip(values)
        .map(|((name, field), value)| {
            let shown = match (field, value) {
                (_, TypedValue::Bool(b)) => json!(b),
                (_, TypedValue::Integer(n)) => json!(n),
                (Field::Choice(options), TypedValue::Choice(at)) => {
                    options.get(*at as usize).map_or(Value::Null, |o| json!(o))
                }
                (_, TypedValue::Choice(at)) => json!(at),
            };
            (name.clone(), shown)
        })
        .collect()
}
