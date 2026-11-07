//! Reading forge content for a job (ADR-0002, ADR-0005, ADR-0047, S3.7).
//!
//! Untrusted input is parsed here, in the broker, never in the engine. The broker asks the forge for
//! an issue of the job's repository with the job's forge token, over GitHub's GraphQL API. That API
//! answers with exactly the fields asked for, so a field the broker did not ask for is a reason to
//! refuse, not something to skip. The answer is held to a size cap and a depth cap before it is
//! parsed, then parsed against closed types, then held to the issue's own caps. Anything that fails is
//! a reason code: the job fails, and the broker carries on. The engine receives the issue only as a
//! [`ForgeIssue`], whose every text is forge-authored by type.

use std::io::Read;

use hyalos_types::provenance::IssueFields;
use hyalos_types::{CommitSha, ForgeIssue, ForgeText, ForgeToken, IssueState, Manifest};
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::model::{HttpTransport, Upstream};
use crate::pulls::PullRequest;

/// GitHub's API, where its GraphQL endpoint is `/graphql`.
pub const GITHUB_API: &str = "https://api.github.com";
/// The most bytes of an answer the broker reads. An issue within its caps is far smaller.
pub const MAX_RESPONSE: usize = 256 * 1024;
/// The deepest nesting of objects and arrays an answer may have. The issue query needs seven.
pub const MAX_DEPTH: usize = 16;
/// The largest issue number GraphQL's 32-bit `Int` can ask for.
const MAX_NUMBER: u64 = i32::MAX as u64;

const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){issue(number:$number){number state title body author{login} labels(first:32){totalCount nodes{name}}}}}";

/// The forge's API.
#[derive(Debug, Clone)]
pub struct Forge {
    transport: Result<HttpTransport, &'static str>,
}

impl Default for Forge {
    fn default() -> Self {
        Self::github()
    }
}

impl Forge {
    /// api.github.com, over TLS to the compiled-in roots (ADR-0039).
    pub fn github() -> Self {
        Self::with_base(GITHUB_API)
    }

    /// Another API host: the one the engine names in `Hello` (ADR-0050), or a test's on loopback.
    pub fn with_base(base: &str) -> Self {
        Self {
            transport: HttpTransport::new(base).map_err(|e| e.0),
        }
    }

    /// Reads issue `number` of the manifest's repository with `token`. `Err` is the code the job fails
    /// with; nothing in it comes from the forge's answer.
    pub fn read_issue(
        &self,
        manifest: &Manifest,
        token: Option<&ForgeToken>,
        number: u64,
    ) -> Result<ForgeIssue, &'static str> {
        if !manifest.actions.contains("read") {
            return Err("forge.action_not_allowed");
        }
        let token = token.ok_or("forge.no_token")?;
        if !(1..=MAX_NUMBER).contains(&number) {
            return Err("forge.bad_request");
        }
        let (owner, name) = manifest.repo.split_once('/').ok_or("forge.bad_request")?;
        let transport = self.transport.as_ref().map_err(|code| *code)?;
        let body = serde_json::json!({
            "query": QUERY,
            "variables": { "owner": owner, "name": name, "number": number },
        });
        let request = Upstream {
            target: "/graphql".into(),
            headers: vec![
                ("authorization".into(), format!("bearer {}", token.expose())),
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "application/json".into()),
                ("user-agent".into(), "hyalos".into()),
            ],
            body: serde_json::to_vec(&body).map_err(|_| "forge.bad_request")?,
        };
        let response = transport.request("POST", &request).map_err(|e| e.0)?;
        match response.status {
            200 => {}
            401 | 403 => return Err("forge.upstream_refused"),
            _ => return Err("forge.upstream_status"),
        }
        let mut answer = Vec::new();
        response
            .body
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut answer)
            .map_err(|_| "forge.unreadable")?;
        parse_issue(&answer)
    }
}

impl Forge {
    /// Opens `pull` on the manifest's repository with `token`, once the broker has checked it
    /// (`pulls::answer`). `Ok` is the pull request's number and page; `Err` is the status and code
    /// the harness is answered with.
    pub(crate) fn open_pull(
        &self,
        manifest: &Manifest,
        token: &ForgeToken,
        pull: &PullRequest,
    ) -> Result<(u64, String), (u16, &'static str)> {
        let transport = self.transport.as_ref().map_err(|code| (502, *code))?;
        let body = serde_json::json!({
            "title": pull.title,
            "body": pull.body.as_deref().unwrap_or_default(),
            "head": pull.head,
            "base": pull.base,
            "draft": pull.draft,
            // Nobody but the job's own branch writes to its head.
            "maintainer_can_modify": false,
        });
        let request = Upstream {
            target: format!("/repos/{}/pulls", manifest.repo),
            headers: vec![
                ("authorization".into(), format!("Bearer {}", token.expose())),
                ("accept".into(), "application/vnd.github+json".into()),
                ("x-github-api-version".into(), "2022-11-28".into()),
                ("content-type".into(), "application/json".into()),
                ("user-agent".into(), "hyalos".into()),
            ],
            body: serde_json::to_vec(&body).map_err(|_| (400, "forge.bad_request"))?,
        };
        let response = transport
            .request("POST", &request)
            .map_err(|e| (502, e.0))?;
        match response.status {
            201 => {}
            401 | 403 => return Err((403, "forge.upstream_refused")),
            404 => return Err((404, "forge.upstream_not_found")),
            // Invalid, or a pull request for this head already exists.
            422 => return Err((422, "forge.pull_rejected")),
            _ => return Err((502, "forge.upstream_status")),
        }
        let mut answer = Vec::new();
        response
            .body
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut answer)
            .map_err(|_| (502, "forge.unreadable"))?;
        let created: serde_json::Value =
            serde_json::from_slice(&answer).map_err(|_| (502, "forge.malformed"))?;
        let number = created["number"].as_u64();
        let url = created["html_url"].as_str().filter(|u| {
            u.len() <= 512
                && (u.starts_with("https://") || u.starts_with("http://"))
                && !u.contains(char::is_control)
        });
        match (number, url) {
            (Some(number), Some(url)) => Ok((number, url.to_string())),
            _ => Err((502, "forge.malformed")),
        }
    }
}

impl Forge {
    /// Merges the pull request the manifest's approval names, by its method and only while the pull
    /// request's head is the approved commit (ADR-0012, ADR-0071). GitHub checks the head itself
    /// (`sha`), so a push after the approval makes it refuse. `Ok` is the merge commit; `Err` is the
    /// code the job fails with, and nothing in it comes from the forge's answer.
    pub fn merge(
        &self,
        manifest: &Manifest,
        token: Option<&ForgeToken>,
    ) -> Result<CommitSha, &'static str> {
        let approval = match &manifest.approval {
            Some(approval) if manifest.actions.contains("merge") && !manifest.tainted => approval,
            _ => return Err("merge.not_approved"),
        };
        let token = token.ok_or("forge.no_token")?;
        if !hyalos_types::merge::valid_repo(&manifest.repo)
            || !hyalos_types::merge::valid_pr(approval.pr)
        {
            return Err("forge.bad_request");
        }
        let transport = self.transport.as_ref().map_err(|code| *code)?;
        let body = serde_json::json!({
            "sha": approval.head_sha.as_str(),
            "merge_method": approval.method.as_str(),
        });
        let request = Upstream {
            target: format!("/repos/{}/pulls/{}/merge", manifest.repo, approval.pr),
            headers: vec![
                ("authorization".into(), format!("Bearer {}", token.expose())),
                ("accept".into(), "application/vnd.github+json".into()),
                ("x-github-api-version".into(), "2022-11-28".into()),
                ("content-type".into(), "application/json".into()),
                ("user-agent".into(), "hyalos".into()),
            ],
            body: serde_json::to_vec(&body).map_err(|_| "forge.bad_request")?,
        };
        // Only an error before anything was sent means GitHub did not merge; after that it may have.
        let response = transport.request("PUT", &request).map_err(|e| match e.0 {
            "transport.unreachable" | "transport.tls_refused" | "transport.bad_request" => e.0,
            _ => "merge.unconfirmed",
        })?;
        match response.status {
            200 => {}
            401 | 403 => return Err("forge.upstream_refused"),
            404 => return Err("forge.upstream_not_found"),
            // Not mergeable: closed, conflicting, failing a required check, or the method is off.
            405 => return Err("merge.not_mergeable"),
            // The head is no longer the approved commit.
            409 => return Err("merge.head_moved"),
            422 => return Err("merge.rejected"),
            _ => return Err("forge.upstream_status"),
        }
        // GitHub has answered 200: it has merged. An answer that cannot be read confirms nothing,
        // so the operator is told to look rather than that nothing happened.
        let mut answer = Vec::new();
        response
            .body
            .take(MAX_MERGE_ANSWER as u64 + 1)
            .read_to_end(&mut answer)
            .map_err(|_| "merge.unconfirmed")?;
        parse_merged(&answer).map_err(|code| match code {
            "merge.not_merged" => code,
            _ => "merge.unconfirmed",
        })
    }
}

/// The most bytes of a merge's answer the broker reads: a commit id, a flag and a short message.
const MAX_MERGE_ANSWER: usize = 16 * 1024;

/// Reads the merge commit from GitHub's answer to a merge. Only `merged: true` and a well-formed
/// commit id are taken; the message is never read. Public so it can be tried on any bytes.
pub fn parse_merged(answer: &[u8]) -> Result<CommitSha, &'static str> {
    if answer.len() > MAX_MERGE_ANSWER {
        return Err("forge.too_large");
    }
    if depth(answer) > MAX_DEPTH {
        return Err("forge.too_deep");
    }
    let reply: serde_json::Value = serde_json::from_slice(answer).map_err(|_| "forge.malformed")?;
    if reply.get("merged").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("merge.not_merged");
    }
    reply
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .and_then(CommitSha::new)
        .ok_or("forge.malformed")
}

/// Parses the forge's answer to the issue query. Public so it can be tried on any bytes.
pub fn parse_issue(answer: &[u8]) -> Result<ForgeIssue, &'static str> {
    if answer.len() > MAX_RESPONSE {
        return Err("forge.too_large");
    }
    if depth(answer) > MAX_DEPTH {
        return Err("forge.too_deep");
    }
    let reply: Reply = serde_json::from_slice(answer).map_err(|e| {
        if e.to_string().starts_with("unknown field") {
            "forge.unknown_field"
        } else {
            "forge.malformed"
        }
    })?;
    if reply.errors.is_some() {
        return Err("forge.refused");
    }
    let issue = reply
        .data
        .ok_or("forge.malformed")?
        .repository
        .ok_or("forge.not_found")?
        .issue
        .ok_or("forge.no_such_issue")?;
    let state = match issue.state.as_str() {
        "OPEN" => IssueState::Open,
        "CLOSED" => IssueState::Closed,
        _ => return Err("forge.malformed"),
    };
    // Every label is kept or the read fails: a partial list would misstate the issue.
    if issue.labels.total_count as usize != issue.labels.nodes.len() {
        return Err("forge.too_large");
    }
    ForgeIssue::new(IssueFields {
        number: issue.number,
        state,
        title: ForgeText::new(issue.title),
        body: ForgeText::new(issue.body),
        author: issue.author.map(|a| ForgeText::new(a.login)),
        labels: issue
            .labels
            .nodes
            .into_iter()
            .map(|l| ForgeText::new(l.name))
            .collect(),
    })
    .map_err(|_| "forge.too_large")
}

/// The deepest nesting of objects and arrays in `json`, read without parsing it, so an answer nested
/// too deeply is refused before a parser recurses into it.
pub fn depth(json: &[u8]) -> usize {
    let (mut depth, mut deepest, mut in_string, mut escaped) = (0usize, 0, false, false);
    for &b in json {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

// GraphQL's answer to the query, and nothing else: an unknown field is refused.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    data: Option<Data>,
    errors: Option<IgnoredAny>,
    #[allow(dead_code)]
    extensions: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    issue: Option<Issue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Issue {
    number: u64,
    state: String,
    title: String,
    body: String,
    author: Option<Author>,
    labels: Labels,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Author {
    login: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Labels {
    total_count: u64,
    nodes: Vec<Label>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_counts_nesting_outside_strings_only() {
        assert_eq!(depth(b"1"), 0);
        assert_eq!(depth(br#"{"a":[1,{"b":2}]}"#), 3);
        assert_eq!(depth(br#"{"a":"[[[[{{{{\"]]]"}"#), 1);
        assert_eq!(depth(&[b'['; 100_000]), 100_000);
    }
}
