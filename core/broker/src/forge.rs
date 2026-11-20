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
use hyalos_types::writers::MAX_EDITORS;
use hyalos_types::{
    CommitSha, ForgeIssue, ForgeText, ForgeToken, IssueState, Manifest, Permission, Writers,
};
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::model::{HttpTransport, Upstream};
use crate::pulls::PullRequest;

/// GitHub's API, where its GraphQL endpoint is `/graphql`.
pub const GITHUB_API: &str = "https://api.github.com";
/// The most bytes of an answer the broker reads. An issue within its caps is far smaller.
pub const MAX_RESPONSE: usize = 256 * 1024;
/// The deepest nesting of objects and arrays an answer may have. The issue query needs eight.
pub const MAX_DEPTH: usize = 16;
/// The most body edits and title renames the issue query asks for, each. An issue with more has
/// writers the broker cannot name, so it is not released by policy (ADR-0103).
const MAX_EDITS: usize = 16;
/// The most bytes of a permission answer the broker reads: a role, and a user it ignores.
const MAX_PERMISSION_ANSWER: usize = 16 * 1024;
/// The largest issue number GraphQL's 32-bit `Int` can ask for.
const MAX_NUMBER: u64 = i32::MAX as u64;

const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){issue(number:$number){number state title body author{__typename login} labels(first:32){totalCount nodes{name}} userContentEdits(first:16){totalCount nodes{editor{__typename login}}} timelineItems(itemTypes:[RENAMED_TITLE_EVENT],first:16){totalCount nodes{... on RenamedTitleEvent{actor{__typename login}}}}}}}";

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
        parse_issue(&self.query_issue(manifest, token, number)?)
    }

    /// Reads issue `number` as [`read_issue`](Self::read_issue) does, and who wrote it: its author,
    /// the editors of its body and whoever renamed it, read in the same query so they wrote this text,
    /// then each one's permission on the repository (ADR-0103). Only the permissions come out; a
    /// writer the broker cannot establish is [`Permission::Unknown`].
    pub fn read_issue_and_writers(
        &self,
        manifest: &Manifest,
        token: Option<&ForgeToken>,
        number: u64,
    ) -> Result<(ForgeIssue, Writers), &'static str> {
        let answer = self.query_issue(manifest, token, number)?;
        let (issue, logins) = parse(&answer)?;
        // A token is there, or the query above had failed.
        let writers = match token {
            Some(token) => self.writers(&manifest.repo, token, &logins),
            None => Writers::unknown(),
        };
        Ok((issue, writers))
    }

    fn query_issue(
        &self,
        manifest: &Manifest,
        token: Option<&ForgeToken>,
        number: u64,
    ) -> Result<Vec<u8>, &'static str> {
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
        read_capped(response.body, MAX_RESPONSE).map_err(|_| "forge.unreadable")
    }

    /// Each writer's permission on `repo`: the author's, and every other writer's once each.
    fn writers(&self, repo: &str, token: &ForgeToken, logins: &Logins) -> Writers {
        let author = match &logins.author {
            Some(login) => self.permission(repo, token, login),
            None => Permission::Unknown,
        };
        let mut others: Vec<&Option<String>> = Vec::new();
        for login in &logins.others {
            let same = |a: &Option<String>, b: &Option<String>| match (a, b) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                _ => false,
            };
            if !same(login, &logins.author) && !others.iter().any(|o| same(o, login)) {
                others.push(login);
            }
        }
        // More writers than it may report, or edits it was not shown: not one it can release.
        let editors = if !logins.complete || others.len() > MAX_EDITORS {
            vec![Permission::Unknown]
        } else {
            others
                .into_iter()
                .map(|login| match login {
                    Some(login) => self.permission(repo, token, login),
                    None => Permission::Unknown,
                })
                .collect()
        };
        Writers { author, editors }
    }

    /// `login`'s permission on `repo`, as GitHub's base role; unknown for anything else.
    fn permission(&self, repo: &str, token: &ForgeToken, login: &str) -> Permission {
        let valid = (1..=39).contains(&login.len())
            && !login.starts_with('-')
            && login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        let Ok(transport) = self.transport.as_ref() else {
            return Permission::Unknown;
        };
        if !valid {
            return Permission::Unknown;
        }
        let target = format!("/repos/{repo}/collaborators/{login}/permission");
        let Ok(response) = transport.request("GET", &rest(token, target, None)) else {
            return Permission::Unknown;
        };
        if response.status != 200 {
            return Permission::Unknown;
        }
        match read_capped(response.body, MAX_PERMISSION_ANSWER) {
            Ok(answer) => parse_permission(&answer),
            Err(_) => Permission::Unknown,
        }
    }
}

/// A call to GitHub's REST API with `token`. A body is JSON.
fn rest(token: &ForgeToken, target: String, body: Option<Vec<u8>>) -> Upstream {
    let mut headers = vec![
        ("authorization".into(), format!("Bearer {}", token.expose())),
        ("accept".into(), "application/vnd.github+json".into()),
        ("x-github-api-version".into(), "2022-11-28".into()),
    ];
    if body.is_some() {
        headers.push(("content-type".into(), "application/json".into()));
    }
    headers.push(("user-agent".into(), "hyalos".into()));
    Upstream {
        target,
        headers,
        body: body.unwrap_or_default(),
    }
}

/// Reads at most one byte more of an answer than `max`, so that a parser that holds it to `max`
/// sees it is too large.
fn read_capped(body: impl Read, max: usize) -> std::io::Result<Vec<u8>> {
    let mut answer = Vec::new();
    body.take(max as u64 + 1).read_to_end(&mut answer)?;
    Ok(answer)
}

/// Reads the role from GitHub's answer to a permission question; unknown if it cannot.
pub fn parse_permission(answer: &[u8]) -> Permission {
    if answer.len() > MAX_PERMISSION_ANSWER || depth(answer) > MAX_DEPTH {
        return Permission::Unknown;
    }
    serde_json::from_slice::<serde_json::Value>(answer)
        .ok()
        .and_then(|v| v.get("permission")?.as_str().map(Permission::from_role))
        .unwrap_or(Permission::Unknown)
}

/// Who wrote an issue, as the query named them: `None` for a deleted account or an actor that is
/// not a user, so a bot cannot pass as a user of the same login. `complete` is false when there were
/// more edits or renames than the query asked for.
struct Logins {
    author: Option<String>,
    others: Vec<Option<String>>,
    complete: bool,
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
        let body = serde_json::to_vec(&body).map_err(|_| (400, "forge.bad_request"))?;
        let request = rest(token, format!("/repos/{}/pulls", manifest.repo), Some(body));
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
        let answer =
            read_capped(response.body, MAX_RESPONSE).map_err(|_| (502, "forge.unreadable"))?;
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
        let body = serde_json::to_vec(&body).map_err(|_| "forge.bad_request")?;
        let target = format!("/repos/{}/pulls/{}/merge", manifest.repo, approval.pr);
        let request = rest(token, target, Some(body));
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
        let answer =
            read_capped(response.body, MAX_MERGE_ANSWER).map_err(|_| "merge.unconfirmed")?;
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
    parse(answer).map(|(issue, _)| issue)
}

fn parse(answer: &[u8]) -> Result<(ForgeIssue, Logins), &'static str> {
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
    let user = |actor: Option<Actor>| actor.filter(|a| a.typename.as_deref() == Some("User"));
    let edits = issue.user_content_edits;
    let renames = issue.timeline_items;
    let logins = Logins {
        author: user(issue.author.clone()).map(|a| a.login),
        complete: edits
            .as_ref()
            .is_some_and(|e| e.total_count as usize == e.nodes.len() && e.nodes.len() <= MAX_EDITS)
            && renames.as_ref().is_some_and(|r| {
                r.total_count as usize == r.nodes.len() && r.nodes.len() <= MAX_EDITS
            }),
        others: edits
            .into_iter()
            .flat_map(|e| e.nodes)
            .map(|n| n.editor)
            .chain(renames.into_iter().flat_map(|r| r.nodes).map(|n| n.actor))
            .map(|actor| user(actor).map(|a| a.login))
            .collect(),
    };
    let issue = ForgeIssue::new(IssueFields {
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
    .map_err(|_| "forge.too_large")?;
    Ok((issue, logins))
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
    author: Option<Actor>,
    labels: Page<Label>,
    // Asked for with the issue (ADR-0103). Left out by an older answer, they leave its writers
    // unknown, so the issue is not released by policy.
    #[serde(default, rename = "userContentEdits")]
    user_content_edits: Option<Page<Edit>>,
    #[serde(default, rename = "timelineItems")]
    timeline_items: Option<Page<Rename>>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Actor {
    #[serde(default, rename = "__typename")]
    typename: Option<String>,
    login: String,
}

/// A connection's nodes, and how many there are in all.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Page<T> {
    total_count: u64,
    nodes: Vec<T>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    editor: Option<Actor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    #[serde(default)]
    actor: Option<Actor>,
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
