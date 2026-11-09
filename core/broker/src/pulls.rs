//! Opening a pull request through the broker (ADR-0048, S3.12).
//!
//! A job asks for a pull request on its socket as it would ask GitHub's REST API:
//! `POST /forge/repos/{owner}/{repo}/pulls` with `title`, `body`, `head`, `base` and `draft`. The
//! broker opens it with the job's forge token only for the manifest's repository, with the `open_pr`
//! action, from a branch inside the manifest's branch prefix, into the manifest's `pr_base`, and only
//! once its title and body have passed the content gate. A refusal is a 403 to the harness and the
//! manifest or content gate's `deny` to the engine, carrying a SHA-256 of what was refused.
//!
//! If the manifest names an issue the job fixes, the broker adds a line saying the pull request
//! closes it, after the harness's text has passed the gate (ADR-0083). The number is the run's own,
//! not the harness's, so the merge closes the issue whatever the harness wrote.

use std::io::{BufRead, Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use hyalos_types::{ForgeToken, Manifest};
use serde::Deserialize;

use crate::content;
use crate::egress::{Gates, ProxyRequest, Reports, inputs_digest, refused};
use crate::forge::Forge;
use crate::git::{read_body, ref_allowed, well_formed_ref};
use crate::model::Response;

/// Where forge calls arrive on a job's socket.
pub const PREFIX: &str = "/forge/";
/// The longest title GitHub takes, in characters.
pub const MAX_TITLE: usize = 256;
/// The longest body GitHub takes, in characters.
pub const MAX_BODY: usize = 65_536;
/// The most bytes of a request the broker reads.
const MAX_REQUEST: usize = 512 * 1024;

/// A pull request as the job asks for it, in GitHub's field names. Any other field is refused.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequest {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub head: String,
    pub base: String,
    #[serde(default)]
    pub draft: bool,
}

/// Reads `owner/name` from a `POST /forge/repos/{owner}/{name}/pulls`. `None` is not a call the
/// broker serves.
pub fn route(method: &str, target: &str) -> Option<String> {
    let rest = target.strip_prefix(PREFIX)?.strip_prefix("repos/")?;
    let mut parts = rest.split('/');
    let (owner, name, call, end) = (parts.next()?, parts.next()?, parts.next()?, parts.next());
    let part = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    (method == "POST" && call == "pulls" && end.is_none() && part(owner) && part(name))
        .then(|| format!("{owner}/{name}"))
}

/// Answers one forge call for the job whose manifest is `manifest`. `secrets` are the credentials the
/// content gate looks for: the job's forge token and the provider keys. `web` is the forge's web
/// host, the one a tainted job's text may link to (ADR-0065).
#[allow(clippy::too_many_arguments)]
pub(crate) fn answer<R: BufRead, W: Write>(
    request: &ProxyRequest,
    reader: &mut R,
    client: &mut W,
    manifest: &Manifest,
    token: Option<&ForgeToken>,
    forge: &Forge,
    web: Option<&str>,
    secrets: &[&str],
    gates: Gates<'_>,
    ended: &AtomicBool,
) -> Response {
    let deny = |reports: &Reports, code: &'static str, inputs: &str| {
        reports.deny(code, inputs_digest(inputs), ended);
        refused(403, code)
    };
    let Some(repo) = route(&request.method, &request.target) else {
        return refused(404, "forge.route_refused");
    };
    if !repo.eq_ignore_ascii_case(&manifest.repo) {
        return deny(
            gates.manifest,
            "forge.repo_not_allowed",
            &repo.to_ascii_lowercase(),
        );
    }
    if !manifest.actions.contains("open_pr") {
        return deny(gates.manifest, "forge.action_not_allowed", "open_pr");
    }
    let Some(token) = token else {
        return refused(403, "forge.no_token");
    };
    let header = |name: &str| {
        request
            .headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    let body = match read_body(header, reader, client, MAX_REQUEST) {
        Ok(body) => body,
        Err((status, _)) => return refused(status, "forge.bad_request"),
    };
    let pull: PullRequest = match serde_json::from_slice(&body) {
        Ok(pull) => pull,
        Err(_) => return refused(400, "forge.bad_request"),
    };
    let text = pull.body.as_deref().unwrap_or_default();
    if pull.title.trim().is_empty()
        || pull.title.chars().count() > MAX_TITLE
        || text.chars().count() > MAX_BODY
    {
        return refused(422, "forge.bad_request");
    }
    // The head is a branch of this repository, inside the prefix; `owner:branch` is another fork's.
    if pull.head.contains(':') || !ref_allowed(manifest, &format!("refs/heads/{}", pull.head)) {
        return deny(gates.manifest, "forge.head_not_allowed", &pull.head);
    }
    if manifest.pr_base.as_deref() != Some(pull.base.as_str()) || !well_formed_ref(&pull.base) {
        return deny(gates.manifest, "forge.base_not_allowed", &pull.base);
    }
    let mut checked = secrets.to_vec();
    checked.push(token.expose());
    // A tainted job's text is published where people and their browsers render it (ADR-0065).
    for part in [pull.title.as_str(), text] {
        let checked = if manifest.tainted {
            content::check_published(part, &checked, web)
        } else {
            content::check(part, &checked)
        };
        if let Err(code) = checked {
            return deny(gates.content, code, &format!("{}\n{text}", pull.title));
        }
    }
    let pull = match manifest.closes {
        Some(issue) => PullRequest {
            body: Some(closing(text, issue)),
            ..pull
        },
        None => pull,
    };
    if pull.body.as_deref().unwrap_or_default().chars().count() > MAX_BODY {
        return refused(422, "forge.bad_request");
    }
    if ended.load(Ordering::SeqCst) {
        return refused(403, "admission.ended");
    }
    match forge.open_pull(manifest, token, &pull) {
        Ok((number, url)) => {
            let answer = serde_json::json!({ "number": number, "html_url": url });
            Response {
                status: 201,
                content_type: "application/json".into(),
                body: Box::new(Cursor::new(answer.to_string().into_bytes())),
                reason: None,
            }
        }
        Err((status, code)) => refused(status, code),
    }
}

/// GitHub's keywords that close an issue when the pull request merges into the default branch.
const KEYWORDS: [&str; 9] = [
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

/// `body` with a line of its own saying it closes `issue`, unless it already says so (ADR-0083).
/// What the harness wrote is kept as it is.
pub fn closing(body: &str, issue: u64) -> String {
    if closes(body, issue) {
        return body.to_string();
    }
    let line = format!("Closes #{issue}");
    if body.trim().is_empty() {
        line
    } else {
        format!("{}\n\n{line}", body.trim_end())
    }
}

/// Whether `body` closes `#issue` with one of GitHub's keywords: the keyword as a word of its own,
/// then an optional colon, then at least one space, then `#` and exactly that number. When in doubt
/// it answers no, so the line is added: a second closing line is harmless, a missing one is not.
fn closes(body: &str, issue: u64) -> bool {
    let lower = body.to_ascii_lowercase();
    let number = issue.to_string();
    let bytes = lower.as_bytes();
    KEYWORDS.iter().any(|keyword| {
        lower.match_indices(keyword).any(|(at, _)| {
            let before = at.checked_sub(1).map(|i| bytes[i]);
            if before.is_some_and(|b| b.is_ascii_alphanumeric()) {
                return false;
            }
            let rest = &lower[at + keyword.len()..];
            let rest = rest.strip_prefix(':').unwrap_or(rest);
            let trimmed = rest.trim_start_matches([' ', '\t']);
            // GitHub's form has a space before the `#`; anything else gets the line added.
            if trimmed.len() == rest.len() {
                return false;
            }
            trimmed
                .strip_prefix('#')
                .and_then(|r| r.strip_prefix(number.as_str()))
                .is_some_and(|r| !r.starts_with(|c: char| c.is_ascii_digit()))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closing_line_is_added_once_and_the_text_kept() {
        assert_eq!(closing("", 24), "Closes #24");
        assert_eq!(closing("  \n", 24), "Closes #24");
        assert_eq!(
            closing("Clarifies the README.\n", 24),
            "Clarifies the README.\n\nCloses #24"
        );
        for already in [
            "Fixes #24",
            "fixed: #24 by rewording",
            "This resolves #24.",
            "CLOSES #24",
        ] {
            assert_eq!(closing(already, 24), already, "{already}");
        }
        // Another issue, a longer number, or a keyword inside a word is not this issue closed.
        for other in [
            "Fixes #240",
            "Fixes #2",
            "Refs #24",
            "prefixes #24",
            "fixes issue 24",
            "closes#24",
        ] {
            assert_eq!(
                closing(other, 24),
                format!("{other}\n\nCloses #24"),
                "{other}"
            );
        }
    }

    #[test]
    fn only_a_post_to_a_repositorys_pulls_is_served() {
        assert_eq!(
            route("POST", "/forge/repos/octo/repo/pulls").as_deref(),
            Some("octo/repo")
        );
        for (method, target) in [
            ("GET", "/forge/repos/octo/repo/pulls"),
            ("POST", "/forge/repos/octo/repo/pulls/1/merge"),
            ("POST", "/forge/repos/octo/repo/issues"),
            ("POST", "/forge/repos/../repo/pulls"),
            ("POST", "/forge/repos/octo/pulls"),
            ("POST", "/forge/graphql"),
        ] {
            assert_eq!(route(method, target), None, "{method} {target}");
        }
    }
}
