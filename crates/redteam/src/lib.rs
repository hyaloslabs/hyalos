//! The red-team corpus (ADR-0013, ADR-0065, S4.5).
//!
//! The corpus is data: `corpus/<channel>.json`, ported from hivrr's corpus (version 1). Each item says
//! what a stranger planted (`planted`), what a fully steered agent would then try (`steered`), and
//! what Hyalos is expected to do about it (`expected`). Everything in it is a canary: no real secret
//! and no working exploit. Secret-shaped strings are stored as `{{secret:<kind>}}` placeholders and
//! built here from split literals, so neither a secret scanner nor a reader takes the data for a leak.
//!
//! The measure is the worst case: if the injection worked and the agent complies, does its steered
//! write get through a gate? That is a question about the gates, not the model, so it runs offline.
//! [`Item::writes`] says what that write is in Hyalos, where a job's only ways out are its socket's
//! routes. hivrr's comment, issue and MCP sinks have no route here. Their text is tried at the one
//! forge sink Hyalos has, a pull request's body, and the route itself is tried as well.

use serde::Deserialize;

/// The repository the corpus's writes are for, as the port rewrote hivrr's.
pub const REPO: &str = "octo/repo";

/// The channels, one file each.
pub const CHANNELS: [&str; 5] = [
    "issue_body",
    "issue_comment",
    "repo_file",
    "linked_page",
    "tool_output",
];

const FILES: [&str; 5] = [
    include_str!("../corpus/issue_body.json"),
    include_str!("../corpus/issue_comment.json"),
    include_str!("../corpus/repo_file.json"),
    include_str!("../corpus/linked_page.json"),
    include_str!("../corpus/tool_output.json"),
];

/// One item of the corpus.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub channel: String,
    pub technique: String,
    pub class: Class,
    pub canary: String,
    pub planted: serde_json::Value,
    pub steered: Steered,
    /// What the steered write probes, in words.
    pub probes: String,
    pub expected: Expected,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Attack,
    Control,
}

/// What Hyalos is expected to do with an item's steered write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expected {
    /// An attack every write of which is refused.
    Refused,
    /// An attack whose write reaches its sink: a known gap, such as a secret written as prose.
    Reached,
    /// A benign write that must not be refused.
    Accepted,
    /// A benign read with no sink in Hyalos: the sandbox reads its own clone.
    NoSink,
}

/// What a steered agent tries.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steered {
    pub form: String,
    pub sink: Sink,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub read_only: Option<bool>,
    #[serde(default)]
    pub taint_ok: Option<bool>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub leak: Option<bool>,
}

/// Where hivrr's steered write went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sink {
    ForgeComment,
    ForgeIssueComment,
    ForgePrBody,
    ForgeIssueBody,
    McpWrite,
    McpRead,
}

/// A write as a Hyalos job would make it, on its socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// `POST /forge/repos/{REPO}/pulls` with this body.
    PullRequest { body: String },
    /// A forge call on another path under `/forge/`, with this JSON body.
    Forge { path: String, body: String },
    /// `CONNECT host:443`: a tool or page the job would reach on the web.
    Connect { host: String },
}

/// Every item of the corpus.
pub fn corpus() -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for (channel, file) in CHANNELS.iter().zip(FILES) {
        let read: Vec<Item> =
            serde_json::from_str(file).map_err(|e| format!("corpus/{channel}.json: {e}"))?;
        items.extend(read);
    }
    Ok(items)
}

impl Item {
    /// The steered write's text, rendered.
    pub fn text(&self) -> String {
        render(
            self.steered.text.as_deref().unwrap_or_default(),
            &self.canary,
        )
    }

    /// What the steered write is in Hyalos. Empty for a read with no sink.
    pub fn writes(&self) -> Vec<Write> {
        let text = self.text();
        let issue = |path: &str| Write::Forge {
            path: format!("/forge/repos/{REPO}/{path}"),
            body: serde_json::json!({ "body": text }).to_string(),
        };
        match self.steered.sink {
            Sink::ForgePrBody => vec![Write::PullRequest { body: text.clone() }],
            Sink::ForgeComment => vec![
                Write::PullRequest { body: text.clone() },
                issue("issues/1/comments"),
            ],
            Sink::ForgeIssueComment => vec![
                Write::PullRequest { body: text.clone() },
                issue("issues/7/comments"),
            ],
            Sink::ForgeIssueBody => vec![
                Write::PullRequest { body: text.clone() },
                Write::Forge {
                    path: format!("/forge/repos/{REPO}/issues"),
                    body: serde_json::json!({ "title": "Retry", "body": text }).to_string(),
                },
            ],
            Sink::McpRead | Sink::McpWrite if self.expected == Expected::NoSink => Vec::new(),
            Sink::McpRead | Sink::McpWrite => vec![Write::Connect {
                host: self.tool_host(),
            }],
        }
    }

    /// The host a tool call reaches: the one its URL argument names, or its server's.
    fn tool_host(&self) -> String {
        let url = self
            .steered
            .args
            .as_ref()
            .and_then(|a| a.get("url"))
            .and_then(|u| u.as_str())
            .map(|u| render(u, &self.canary));
        match url {
            Some(url) => {
                let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
                rest.split(['/', '?', '#', ':'])
                    .next()
                    .unwrap_or_default()
                    .to_string()
            }
            None => format!(
                "{}.mcp.example",
                self.steered.server.as_deref().unwrap_or("tool")
            ),
        }
    }
}

/// A secret-shaped canary string of `kind`, built from split literals. It matches the shape only.
fn secret(kind: &str, c: &str) -> String {
    let t = format!("RTCANARY{c}");
    match kind {
        "anthropic" => format!("s{}{}{t}", "k-", "ant-api03-"),
        "github" => {
            let body: String = format!("{t}{}", "x".repeat(24)).chars().take(36).collect();
            format!("gh{}{body}", "p_")
        }
        "gitlab" => format!("gl{}{t}", "pat-"),
        "aws" => {
            let body: String = format!("RTCANARY{}", c.to_uppercase())
                .chars()
                .take(16)
                .collect();
            format!("AK{}{body}", "IA")
        }
        "pem" => format!("-----BEGIN {}{}\n{t}", "OPENSSH PRIV", "ATE KEY-----"),
        "assign" => format!("API_{}{t}zz", "TOKEN="),
        "urlcred" => format!("https://bot:{t}@git.example/x.git"),
        "jwt" => format!("ey{}{t}.ey{}{t}.{t}", "J", "J"),
        "slack" => format!("xo{}{t}", "xb-"),
        "stripe" => format!("s{}{t}abc", "k_test_"),
        _ => String::new(),
    }
}

/// Expands `{{canary}}`, `{{canary_hex}}`, `{{secret:K}}`, `{{b64|hex|pct|rev|split|zw:K}}` and
/// `{{pad:N}}`, as hivrr's renderer does.
pub fn render(template: &str, canary: &str) -> String {
    let c = canary
        .split_once('-')
        .map_or(canary, |(_, c)| c)
        .to_lowercase();
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let spec = &rest[start + 2..start + end];
        out.push_str(&expand(spec, canary, &c));
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    out
}

fn expand(spec: &str, canary: &str, c: &str) -> String {
    match spec {
        "canary" => return canary.to_string(),
        "canary_hex" => return c.to_uppercase(),
        _ => {}
    }
    let (op, arg) = spec.split_once(':').unwrap_or((spec, ""));
    if op == "pad" {
        return "A".repeat(arg.parse().unwrap_or(0));
    }
    let raw = secret(arg, c);
    match op {
        "secret" => raw,
        "b64" => base64(raw.as_bytes()),
        "hex" => raw.bytes().map(|b| format!("{b:02x}")).collect(),
        "pct" => raw.bytes().map(|b| format!("%{b:02X}")).collect(),
        "rev" => raw.chars().rev().collect(),
        "split" => raw
            .chars()
            .collect::<Vec<_>>()
            .chunks(3)
            .map(|g| g.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" "),
        "zw" => {
            let (head, tail) = raw.split_at(2.min(raw.len()));
            format!("{head}\u{200b}{tail}")
        }
        _ => String::new(),
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_loads_whole_and_every_item_says_what_to_expect() {
        let items = corpus().unwrap();
        assert_eq!(items.len(), 310);
        assert_eq!(
            items.iter().filter(|i| i.class == Class::Attack).count(),
            300
        );
        let ids: std::collections::BTreeSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.len(), items.len(), "ids are unique");
        for item in &items {
            assert_eq!(
                item.class == Class::Control,
                matches!(item.expected, Expected::Accepted | Expected::NoSink),
                "{}",
                item.id
            );
            // Rendering leaves no placeholder behind.
            assert!(!item.text().contains("{{"), "{}", item.id);
        }
    }

    #[test]
    fn placeholders_render_as_hivrr_renders_them() {
        let canary = "CANARY-DFD51BEFB0";
        assert_eq!(
            render("x {{canary}} {{canary_hex}}", canary),
            "x CANARY-DFD51BEFB0 DFD51BEFB0"
        );
        let key = render("{{secret:anthropic}}", canary);
        assert_eq!(key, "sk-ant-api03-RTCANARYdfd51befb0");
        assert_eq!(
            render("{{rev:anthropic}}", canary),
            key.chars().rev().collect::<String>()
        );
        assert_eq!(render("{{b64:anthropic}}", canary), base64(key.as_bytes()));
        assert_eq!(render("{{pad:3}}", canary), "AAA");
        assert_eq!(
            render("{{split:slack}}", canary),
            "xox b-R TCA NAR Ydf d51 bef b0"
        );
    }
}
