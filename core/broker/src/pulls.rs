//! Opening a pull request through the broker (ADR-0048, S3.12).
//!
//! A job asks for a pull request on its socket as it would ask GitHub's REST API:
//! `POST /forge/repos/{owner}/{repo}/pulls` with `title`, `body`, `head`, `base` and `draft`. The
//! broker opens it with the job's forge token only for the manifest's repository, with the `open_pr`
//! action, from a branch inside the manifest's branch prefix, into the manifest's `pr_base`, and only
//! once its title and body have passed the content gate. A refusal is a 403 to the harness and the
//! manifest or content gate's `deny` to the engine, carrying a SHA-256 of what was refused.

use std::io::{BufRead, Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use hyalos_types::{ForgeToken, Manifest};
use serde::Deserialize;

use crate::content;
use crate::egress::{ProxyRequest, Reports, inputs_digest, refused};
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

/// Where one job's refusals are reported.
pub(crate) struct Gates<'a> {
    pub manifest: &'a Reports,
    pub content: &'a Reports,
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
/// content gate looks for: the job's forge token and the provider keys.
#[allow(clippy::too_many_arguments)]
pub(crate) fn answer<R: BufRead, W: Write>(
    request: &ProxyRequest,
    reader: &mut R,
    client: &mut W,
    manifest: &Manifest,
    token: Option<&ForgeToken>,
    forge: &Forge,
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
    for part in [pull.title.as_str(), text] {
        if let Err(code) = content::check(part, &checked) {
            return deny(gates.content, code, &format!("{}\n{text}", pull.title));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
