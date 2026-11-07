//! `hyalos merge <owner/repo> <pr> --head <commit>`: the operator approves a merge, and Hyalos
//! performs it (ADR-0012, ADR-0071).
//!
//! The operator names the commit they reviewed. The approval is recorded as `merge.approved`, signed
//! with the operator's key after their passphrase (ADR-0070), in a task of its own. One job then runs
//! on that task, in the broker with no sandbox: its manifest is untainted, asks for `merge` alone and
//! carries the approval, which the engine mints only from that event and the policy allows only with
//! it. The engine mints the job a token from the GitHub App with the merge's permissions; the broker
//! asks GitHub to merge with `sha` set to the approved commit, so GitHub itself refuses if the pull
//! request's head moved; the token is revoked when the job ends. Nothing the forge wrote is shown or
//! recorded: the pull request is named by its number, and the answer is a commit id or a code.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use hyalos_engine::{Decider, Engine};
use hyalos_types::merge::{valid_pr, valid_repo};
use hyalos_types::provenance::Provenance;
use hyalos_types::wire::{ToBroker, ToEngine};
use hyalos_types::{
    Attestation, Budget, Clock, CommitSha, CredentialTier, Digest, Event, ForgeToken, JobId,
    JobOutcome, MANIFEST_VERSION, Manifest, MergeMethod, Origin, ReasonCode, Role, StepInputs,
    SubstrateName, TaskId, Timestamp,
};

use crate::clock::SystemClock;
use crate::github::GitHubApp;
use crate::launch::{BrokerProcess, JobDir, fresh_manifest_key};
use crate::operator;
use crate::passphrase;
use crate::paths::{self, Env};

const USAGE: &str = "usage: hyalos merge <owner/repo> <pr> --head <commit> [--method merge|squash|rebase] [--reason <code>] [--passphrase-stdin]
  approves merging pull request <pr> of <owner/repo> at <commit>, the head you reviewed, as your
  decision signed with your approval key after your passphrase, then merges it with a token minted
  for this merge alone. GitHub refuses if the pull request's head is no longer <commit>.
  --method is how it is merged (default merge); --reason is a short code (default merge.operator)";

/// How long the merge's manifest is valid.
const MERGE_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// How long the broker has to start and to answer.
const BROKER_WAIT: Duration = Duration::from_secs(30);
const MERGE_WAIT: Duration = Duration::from_secs(120);

/// The codes `job.ended` records for a merge job; a broker's refusal is recorded with its own code.
pub mod why {
    /// GitHub merged the pull request.
    pub const MERGED: &str = "merge.merged";
    /// No manifest was minted for the merge.
    pub const NO_MANIFEST: &str = "merge.no_manifest";
    /// The App minted no token for the merge.
    pub const NO_TOKEN: &str = "merge.no_token";
    /// The broker did not start or did not take the job.
    pub const BROKER_NOT_STARTED: &str = "merge.broker_not_started";
}

/// What the operator approves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    pub repo: String,
    pub pr: u64,
    pub head: CommitSha,
    pub method: MergeMethod,
    pub reason: ReasonCode,
}

/// Reads `<owner/repo> <pr> --head <commit> [--method m] [--reason code]`, with the passphrase flag
/// already taken out.
pub fn parse(args: &[String]) -> Result<MergeRequest, String> {
    let mut positional = Vec::new();
    let (mut head, mut method, mut reason) = (None, MergeMethod::Merge, "merge.operator");
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || rest.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--head" => {
                let text = value()?;
                head = Some(CommitSha::new(text).ok_or_else(|| {
                    format!(
                        "{text} is not a commit id: give the full 40 (or 64) lowercase hex digits, as `git rev-parse` prints them"
                    )
                })?);
            }
            "--method" => {
                let text = value()?;
                method = MergeMethod::from_name(text)
                    .ok_or_else(|| format!("{text} is not merge, squash or rebase"))?;
            }
            "--reason" => reason = value()?,
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => positional.push(arg.as_str()),
        }
    }
    let [repo, pr] = positional.as_slice() else {
        return Err("name the repository and the pull request".into());
    };
    if !valid_repo(repo) {
        return Err(format!("{repo} is not owner/name"));
    }
    let pr = pr
        .parse()
        .ok()
        .filter(|n| valid_pr(*n))
        .ok_or_else(|| format!("{pr} is not a pull request number"))?;
    Ok(MergeRequest {
        repo: (*repo).to_string(),
        pr,
        head: head.ok_or("--head names the commit you reviewed; it is required")?,
        method,
        reason: ReasonCode::new(reason).ok_or_else(|| format!("{reason} is not a short code"))?,
    })
}

pub fn run(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let mut out = std::io::stdout();
    match merge_with(args, &env, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(code)
        }
    }
}

/// `hyalos merge`, writing what happens to `out`.
pub fn merge_with(args: &[String], env: Env<'_>, out: &mut dyn Write) -> Result<(), (u8, String)> {
    let (args, source) = passphrase::source(args);
    let request = parse(&args).map_err(|e| (2, format!("{e}\n{USAGE}")))?;
    let failed = |e: String| (1, e);
    let config_dir = paths::config_dir(env).map_err(failed)?;
    let runtime_dir = paths::runtime_dir(env).map_err(failed)?;
    // Everything that can be checked without the passphrase is checked first.
    let app = GitHubApp::load(&config_dir).map_err(|e| failed(e.to_string()))?;
    let _ = writeln!(
        out,
        "merge {}#{} at {} by {}",
        request.repo,
        request.pr,
        request.head,
        request.method.as_str()
    );
    let mut engine = operator::engine(env, source).map_err(failed)?;
    // Other processes write the same log; each write follows the latest event.
    engine.stamp_forward();
    let exe = std::env::current_exe().map_err(|e| failed(e.to_string()))?;
    merge_job(&mut engine, &app, &request, &exe, &runtime_dir, out).map_err(failed)
}

/// Why the merge job failed, as `job.ended` records it and as the operator reads it.
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

fn code(text: &str) -> ReasonCode {
    ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
}

/// Records the approval in a task of its own, runs the one merge job on it with `broker_exe` as the
/// broker, revokes the job's token and ends the job and the task, whatever happened.
pub fn merge_job(
    engine: &mut Engine,
    app: &GitHubApp,
    request: &MergeRequest,
    broker_exe: &Path,
    runtime_dir: &Path,
    out: &mut dyn Write,
) -> Result<(), String> {
    let now = || SystemClock.now();
    let task = engine
        .submit(
            now(),
            Origin::Engine,
            Event::TaskCreated {
                request: request_digest(request),
            },
        )
        .map_err(|e| e.to_string())?;
    let task = TaskId::from_seq(task.seq);
    let approval = match engine.approve_merge(
        now(),
        task,
        &request.repo,
        request.pr,
        request.head.clone(),
        request.method,
        Decider::Human,
        request.reason.clone(),
    ) {
        Ok(head) => head.seq,
        Err(e) => {
            let _ = engine.submit(now(), Origin::Engine, Event::TaskCompleted { task });
            return Err(format!("the approval was not recorded: {e}"));
        }
    };
    let _ = writeln!(
        out,
        "task {}: merge approved at event {approval}, signed by you",
        task.seq()
    );
    let job = engine
        .submit(
            now(),
            Origin::Engine,
            Event::JobStarted {
                task,
                substrate: SubstrateName::new("broker").unwrap_or_else(SubstrateName::unrecorded),
                credentials: CredentialTier::BrokerHeld,
                attestation: Attestation::None(code("attestation.no_sandbox")),
            },
        )
        .map_err(|e| e.to_string())?;
    let job = JobId::from_seq(job.seq);
    let mut token = None;
    let merged = attempt(
        engine,
        app,
        request,
        approval,
        job,
        broker_exe,
        runtime_dir,
        &mut token,
    );
    let revoked = match &token {
        None => Ok(()),
        Some(token) => revoke(app, job, token, out),
    };
    let (outcome, reason) = match &merged {
        Ok(_) => (JobOutcome::Succeeded, code(why::MERGED)),
        Err(failure) => (JobOutcome::Failed, failure.reason.clone()),
    };
    let ended = engine.submit_all(
        now(),
        &[
            (
                Origin::Engine,
                Event::JobEnded {
                    job,
                    outcome,
                    reason,
                },
            ),
            (Origin::Engine, Event::TaskCompleted { task }),
        ],
    );
    if let Err(e) = &ended {
        let _ = writeln!(out, "job {}: recording its end failed: {e}", job.seq());
    }
    match merged {
        Ok(sha) => {
            let _ = writeln!(out, "job {}: merged {} as {sha}", job.seq(), request.head);
            revoked?;
            ended.map(|_| ()).map_err(|e| e.to_string())
        }
        Err(failure) => Err(format!(
            "job {}: not merged ({}): {}",
            job.seq(),
            failure.reason,
            failure.why
        )),
    }
}

/// Mints the merge job's manifest and token, starts the broker and asks it to merge. The token, once
/// minted, is left in `token` so the caller revokes it whatever happened after.
#[allow(clippy::too_many_arguments)]
fn attempt(
    engine: &Engine,
    app: &GitHubApp,
    request: &MergeRequest,
    approval: u64,
    job: JobId,
    broker_exe: &Path,
    runtime_dir: &Path,
    token: &mut Option<ForgeToken>,
) -> Result<CommitSha, Failure> {
    let now = SystemClock.now();
    let millis = u64::try_from(MERGE_DEADLINE.as_millis()).unwrap_or(u64::MAX);
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        job,
        role: Role::Deterministic,
        tainted: false,
        repo: request.repo.clone(),
        branch_prefix: String::new(),
        pr_base: None,
        actions: ["merge".to_string()].into(),
        egress_hosts: Default::default(),
        tools: Default::default(),
        data_flows: Default::default(),
        budget: Budget { tokens: 0 },
        deadline: Timestamp::from_millis(now.as_millis().saturating_add(millis)),
        approval: Some(hyalos_types::Approval {
            event: approval,
            pr: request.pr,
            head_sha: request.head.clone(),
            method: request.method,
        }),
        handles: Default::default(),
        result: None,
    };
    let key = fresh_manifest_key()
        .map_err(|e| Failure::new(why::NO_MANIFEST, format!("no manifest key: {e}")))?;
    let signed = engine
        .mint_manifest(
            &manifest,
            &StepInputs::of([Provenance::Operator]),
            &key,
            now,
        )
        .map_err(|e| Failure::new(why::NO_MANIFEST, format!("no manifest: {e}")))?;
    let minted = app
        .mint(&manifest, now)
        .map_err(|e| Failure::new(why::NO_TOKEN, format!("no forge token: {e}")))?
        .ok_or_else(|| Failure::new(why::NO_TOKEN, "the merge asked for no forge access"))?;
    *token = Some(minted.token.clone());
    let not_started = |text: String| Failure::new(why::BROKER_NOT_STARTED, text);
    paths::make_private_dir(runtime_dir).map_err(not_started)?;
    let mut broker = BrokerProcess::launch_configured(
        broker_exe,
        runtime_dir,
        &key.public(),
        None,
        Some(app.forge_bases()),
        BROKER_WAIT,
    )
    .map_err(|e| not_started(format!("the broker did not start: {e}")))?;
    let job_dir =
        JobDir::create(runtime_dir).map_err(|e| not_started(format!("no job directory: {e}")))?;
    let bytes = signed.to_bytes().map_err(|e| not_started(e.to_string()))?;
    broker
        .send(&ToBroker::StartJob {
            job,
            role: Role::Deterministic,
            manifest: bytes,
            socket: job_dir.socket().to_string_lossy().into_owned(),
            forge_token: Some(minted.token),
        })
        .map_err(|e| not_started(format!("the broker did not take the job: {e}")))?;
    let merged = match broker.recv() {
        Ok(Some(ToEngine::Admitted { job: admitted })) if admitted == job => {
            broker.merge(job, MERGE_WAIT).map_err(|reason| Failure {
                why: format!("the broker did not merge ({reason})"),
                reason,
            })
        }
        Ok(Some(ToEngine::Refused { reason, .. })) => Err(Failure {
            why: format!("the broker refused the job ({reason})"),
            reason,
        }),
        Ok(other) => Err(not_started(format!("the broker answered {other:?}"))),
        Err(e) => Err(not_started(format!("the broker did not answer: {e}"))),
    };
    if broker.is_open() {
        let _ = broker.send(&ToBroker::EndJob { job });
        let _ = broker.send(&ToBroker::Shutdown);
    }
    drop(job_dir);
    merged
}

fn revoke(
    app: &GitHubApp,
    job: JobId,
    token: &ForgeToken,
    out: &mut dyn Write,
) -> Result<(), String> {
    let revoked = app.revoke(token).map_err(|e| e.to_string());
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
}

/// The digest the merge's task records of what was asked for.
fn request_digest(request: &MergeRequest) -> Digest {
    let text = format!(
        "merge\n{}\n{}\n{}\n{}\n",
        request.repo,
        request.pr,
        request.head,
        request.method.as_str()
    );
    let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes());
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(hash.as_ref());
    Digest::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_merge_names_a_repo_a_number_and_the_commit_reviewed() {
        let request = parse(&args(&format!(
            "octo/repo 42 --head {HEAD} --method squash"
        )))
        .unwrap();
        assert_eq!(request.repo, "octo/repo");
        assert_eq!(request.pr, 42);
        assert_eq!(request.head.as_str(), HEAD);
        assert_eq!(request.method, MergeMethod::Squash);
        assert_eq!(request.reason.as_str(), "merge.operator");
        assert_eq!(
            parse(&args(&format!("--head {HEAD} octo/repo 7")))
                .unwrap()
                .method,
            MergeMethod::Merge
        );
    }

    #[test]
    fn anything_unclear_is_refused_before_the_passphrase() {
        for bad in [
            "octo/repo 42".to_string(),
            format!("octo/repo 42 --head {}", &HEAD[..12]),
            format!("octo/repo 42 --head {}", HEAD.to_uppercase()),
            format!("octo/repo 0 --head {HEAD}"),
            format!("octo 42 --head {HEAD}"),
            format!("octo/repo 42 --head {HEAD} --method ff"),
            format!("octo/repo 42 --head {HEAD} --force"),
            format!("octo/repo 42 43 --head {HEAD}"),
            format!("octo/repo 42 --head {HEAD} --reason Not-A-Code!"),
            "octo/repo 42 --head".to_string(),
        ] {
            assert!(parse(&args(&bad)).is_err(), "{bad}");
        }
    }
}
