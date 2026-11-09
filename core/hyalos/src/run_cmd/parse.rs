//! The command line, its checks, and what a run stores of it so it can resume (ADR-0069).

use super::*;

pub fn parse(args: &[String]) -> Result<Request, String> {
    let mut request = Request {
        repo: String::new(),
        issue: String::new(),
        issue_number: None,
        base: "main".into(),
        image: DEFAULT_IMAGE.into(),
        harness: Harness::Codex,
        budget_tokens: 200_000,
        minutes: 30,
        egress_hosts: BTreeSet::new(),
        step_model: None,
        release_issue: false,
        await_release: false,
        nonce: None,
        skills: BTreeSet::new(),
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
            "--issue-number" => request.issue_number = Some(number(value()?)?),
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
            "--step-model" => request.step_model = Some(value()?),
            "--release-issue" => request.release_issue = true,
            "--await-release" => request.await_release = true,
            "--skill" => {
                let name = PluginName::new(&value()?).ok_or(
                    "--skill is an installed skill's name: lowercase letters, digits and hyphens",
                )?;
                request.skills.insert(name);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    check(&request)?;
    Ok(request)
}

/// The checks every request passes, from the command line or from the driving agent: a repository
/// and a branch name are identifiers, never text (ADR-0069).
pub fn check(request: &Request) -> Result<(), String> {
    if !is_repo(&request.repo) {
        return Err("--repo is owner/name".into());
    }
    if !is_branch(&request.base) {
        return Err("--base is a branch name: letters, digits, '.', '_', '-' and '/'".into());
    }
    if request.release_issue && request.issue_number.is_none() {
        return Err("--release-issue releases the issue of --issue-number".into());
    }
    if request.await_release && request.issue_number.is_none() {
        return Err("--await-release waits for the release of the issue of --issue-number".into());
    }
    if request.await_release && request.release_issue {
        return Err(
            "--await-release and --release-issue are two ways to release the issue; give one"
                .into(),
        );
    }
    if !request
        .egress_hosts
        .iter()
        .all(|h| hyalos_types::capability::Field::EgressHosts.accepts(h))
    {
        return Err(
            "--allow-host is a host name in lower case, such as docs.rs: no port, wildcard, scheme or address (ADR-0085)"
                .into(),
        );
    }
    if request.issue_number.is_none() && request.issue.trim().is_empty() {
        return Err("the issue text is empty; give --issue, --issue-file or --issue-number".into());
    }
    Ok(())
}

/// `owner/name`, each part of letters, digits, `-`, `_` and `.`, and neither `.` nor `..`.
pub fn is_repo(repo: &str) -> bool {
    let part_ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    // GitHub's own limits: an owner of at most 39 characters, a name of at most 100.
    repo.split_once('/').is_some_and(|(owner, name)| {
        part_ok(owner) && part_ok(name) && owner.len() <= 39 && name.len() <= 100
    })
}

/// A branch git and GitHub accept, of plain characters only: at most 100, no `..`, no leading `-`,
/// `/` or `.`, and no trailing `/`, `.` or `.lock`.
pub fn is_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 100
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b))
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.starts_with(['-', '/', '.'])
        && !branch.ends_with(['/', '.'])
        && !branch.ends_with(".lock")
}

/// A request as a run document stores it (ADR-0069): what `task.created` records the SHA-256 of, and
/// what a resumed run is rebuilt from. Every field the run acts on is here, so a changed document does
/// not match its task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stored {
    pub version: u32,
    pub repo: String,
    pub issue: String,
    pub issue_number: Option<u64>,
    pub base: String,
    pub image: String,
    /// `codex`, `claude-code`, or `command` for a test's command, which a document never runs.
    pub harness: String,
    pub budget_tokens: u64,
    pub minutes: u64,
    pub egress_hosts: BTreeSet<String>,
    pub step_model: Option<String>,
    pub release_issue: bool,
    /// Left out when false, so a document written before it has the same digest (ADR-0078).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub await_release: bool,
    pub nonce: Option<String>,
    /// Left out when empty, so a document written before it has the same digest.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub skills: BTreeSet<PluginName>,
}

/// The version of [`Stored`] written today.
pub const STORED_VERSION: u32 = 1;

impl Request {
    pub fn stored(&self) -> Stored {
        Stored {
            version: STORED_VERSION,
            repo: self.repo.clone(),
            issue: self.issue.clone(),
            issue_number: self.issue_number,
            base: self.base.clone(),
            image: self.image.clone(),
            harness: match self.harness {
                Harness::Codex => "codex",
                Harness::ClaudeCode => "claude-code",
                Harness::Command(_) => "command",
            }
            .into(),
            budget_tokens: self.budget_tokens,
            minutes: self.minutes,
            egress_hosts: self.egress_hosts.clone(),
            step_model: self.step_model.clone(),
            release_issue: self.release_issue,
            await_release: self.await_release,
            nonce: self.nonce.clone(),
            skills: self.skills.clone(),
        }
    }

    /// The request a run document holds, checked as one from the command line is.
    pub fn from_stored(stored: Stored) -> Result<Self, String> {
        if stored.version != STORED_VERSION {
            return Err(format!(
                "run document version {} is not one this build reads",
                stored.version
            ));
        }
        let harness = match stored.harness.as_str() {
            "codex" => Harness::Codex,
            "claude-code" => Harness::ClaudeCode,
            other => return Err(format!("a run document cannot run the harness {other}")),
        };
        let request = Self {
            repo: stored.repo,
            issue: stored.issue,
            issue_number: stored.issue_number,
            base: stored.base,
            image: stored.image,
            harness,
            budget_tokens: stored.budget_tokens,
            minutes: stored.minutes,
            egress_hosts: stored.egress_hosts,
            step_model: stored.step_model,
            release_issue: stored.release_issue,
            await_release: stored.await_release,
            nonce: stored.nonce,
            skills: stored.skills,
        };
        check(&request)?;
        Ok(request)
    }
}

/// The digest a task records of what was asked for: SHA-256 of the request's stored form.
pub fn request_digest(request: &Request) -> Digest {
    let bytes = serde_json::to_vec(&request.stored()).unwrap_or_default();
    let hash = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_ref());
    Digest::from_bytes(out)
}
