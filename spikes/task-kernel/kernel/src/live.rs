//! The real world, held by the monitor: one model provider, reached only through the attestation
//! tool and the customer's notary, and one notification channel. This process never holds the
//! model credential. Only the tool reads it.

use crate::{Reason, sha};
use serde_json::Value;
use std::{
    env, fs,
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const LABELS: [&str; 4] = ["bug", "feature", "security", "other"];
const FENCE: &str = "~~~~";
/// The tool holds the 30-second session deadline. This ends its process if it is still running.
const TOOL_LIMIT: Duration = Duration::from_secs(35);
/// More than the tool's one line can be.
const MAX_SAID: u64 = 64 * 1024;
/// What the tool needs to reach the notary. A run with any of them missing is refused.
const NOTARY_SETTINGS: [&str; 4] = ["HYALOS_NOTARY_ADDRESS", "HYALOS_NOTARY_TRANSPORT_PUBLIC", "HYALOS_NOTARY_SIGNER_PUBLIC", "HYALOS_OPERATOR_TRANSPORT_KEY"];

/// The attestation tool, where its presentations go, and the settings it is started with.
pub(crate) struct Attest {
    pub(crate) tool: String,
    pub(crate) dir: String,
    pub(crate) settings: Vec<(String, String)>,
}

impl Attest {
    /// `None` unless the tool and every notary setting are configured.
    pub(crate) fn from_env(record: &str) -> Option<Self> {
        let tool = env::var("HYALOS_ATTEST").ok().filter(|tool| !tool.is_empty())?;
        let settings = NOTARY_SETTINGS
            .iter()
            .map(|name| Some((name.to_string(), env::var(name).ok().filter(|value| !value.is_empty())?)))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { tool, dir: format!("{record}.attest"), settings })
    }
}

pub(crate) struct Live {
    repo: String,
    issue: u64,
    /// With no notary configured, every model call is refused.
    attest: Option<Attest>,
    asked: usize,
    /// The run's intent hash, once its intent link is in the public log. Every attested call
    /// names it, with the call's position, inside the attested request.
    intent: String,
}

/// The reason for a tool that failed, from the one word it printed. Anything else is the reason
/// for a tool that failed in a way it did not name.
fn refusal(said: &[u8]) -> Reason {
    match said.strip_suffix(b"\n").unwrap_or(said) {
        b"unreachable" => Reason::NotaryUnreachable,
        b"deadline" => Reason::NotaryDeadline,
        b"notary-key" => Reason::WrongNotaryKey,
        b"release" => Reason::ReleaseMismatch,
        b"provider" => Reason::ProviderRefused,
        _ => Reason::ModelUnavailable,
    }
}

/// Runs the tool on one question. `Err` when it could not be run or outlived its limit.
fn run_tool(mut command: Command, question: &[u8]) -> Result<(bool, Vec<u8>), Reason> {
    let no = |_| Reason::ModelUnavailable;
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(no)?;
    let written = child.stdin.take().ok_or(Reason::ModelUnavailable).and_then(|mut stdin| stdin.write_all(question).map_err(no));
    let stdout = child.stdout.take().ok_or(Reason::ModelUnavailable);
    let (Ok(()), Ok(stdout)) = (written, stdout) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Reason::ModelUnavailable);
    };
    let reader = std::thread::spawn(move || {
        let mut said = Vec::new();
        stdout.take(MAX_SAID).read_to_end(&mut said).map(|_| said)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TOOL_LIMIT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Reason::ModelUnavailable);
            }
        }
    };
    let said = reader.join().map_err(|_| Reason::ModelUnavailable)?.map_err(no)?;
    Ok((status.success(), said))
}

impl Live {
    pub(crate) fn new(repo: &str, issue: u64, attest: Option<Attest>) -> Option<Self> {
        let repo_ok = repo.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b));
        repo_ok.then(|| Self { repo: repo.to_string(), issue, attest, asked: 0, intent: String::new() })
    }

    /// Names the run every later attested call is made for.
    pub(crate) fn bind(&mut self, intent: &str) {
        self.intent = intent.to_string();
    }

    /// One question, one typed answer that names its presentation. Anything else is a refusal,
    /// and a call is never retried: a retry would take the next sequence number.
    pub(crate) fn infer(&mut self, question: &[u8]) -> Result<(i32, String, String, String), Reason> {
        std::str::from_utf8(question).map_err(|_| Reason::ModelRefused)?;
        let attest = self.attest.as_ref().ok_or(Reason::NotaryUnreachable)?;
        fs::create_dir_all(&attest.dir).map_err(|_| Reason::ModelUnavailable)?;
        let position = self.asked.to_string();
        let file = format!("{}/{:03}.presentation", attest.dir, self.asked);
        self.asked += 1;
        let mut command = Command::new(&attest.tool);
        command.args(["ask", &file, &self.intent, &position]).envs(attest.settings.iter().map(|(name, value)| (name, value)));
        let (succeeded, output) = run_tool(command, question)?;
        if !succeeded {
            return Err(refusal(&output));
        }
        let said: Value = serde_json::from_slice(&output).map_err(|_| Reason::ModelRefused)?;
        let label = said["label"].as_i64().filter(|l| (0..LABELS.len() as i64).contains(l)).ok_or(Reason::ModelRefused)?;
        let named = |key: &str| said[key].as_str().unwrap_or_default().to_string();
        // An answer counts only with the presentation that attests it.
        let presentation = named("presentation_sha256");
        let kept = fs::read(&file).map_err(|_| Reason::ModelRefused)?;
        if presentation.is_empty() || sha(&kept) != presentation {
            return Err(Reason::ModelRefused);
        }
        Ok((label as i32, named("model"), named("response_id"), presentation))
    }

    /// Posts the digest as one issue comment, inside a fixed template the task cannot change. The
    /// body sits in a code block, so nothing in it can mention a user or render as a link.
    pub(crate) fn notify(&self, body: &[u8]) -> Result<String, Reason> {
        let text = std::str::from_utf8(body).map_err(|_| Reason::EffectFailed)?;
        if text.contains(FENCE) {
            return Err(Reason::EffectFailed);
        }
        let comment = format!("Digest sent by one bounded task on the hyalOS kernel spike.\n\n{FENCE}text\n{text}\n{FENCE}\n");
        let mut child = Command::new("gh")
            .args(["issue", "comment", &self.issue.to_string(), "--repo", &self.repo, "--body-file", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Reason::EffectFailed)?;
        child.stdin.take().ok_or(Reason::EffectFailed)?.write_all(comment.as_bytes()).map_err(|_| Reason::EffectFailed)?;
        let output = child.wait_with_output().map_err(|_| Reason::EffectFailed)?;
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && url.starts_with("https://github.com/")).then_some(url).ok_or(Reason::EffectFailed)
    }
}
