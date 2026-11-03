//! What Hyalos ships: the schema, the baseline, and the typed question the engine asks (ADR-0008).
//!
//! The schema and the baseline are files in `policies/`, included at build time, so they are readable
//! and diffable and cannot be replaced at run time. The engine asks with a [`PolicyRequest`]: facts as
//! types, written out as Cedar entities here, so no caller hand-builds JSON.

/// The shipped Cedar schema.
pub const SCHEMA: &str = include_str!("../policies/schema.cedarschema");

/// The shipped baseline policies.
pub const BASELINE: &str = include_str!("../policies/baseline.cedar");

/// How much an action matters (ADR-0012). The schema puts each action in exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Decided by policy.
    Routine,
    /// Deterministic checks plus a verifier, optionally a human.
    Escalated,
    /// Always a human.
    Irreversible,
}

impl Tier {
    /// The action group in the schema that stands for this tier.
    pub fn group(self) -> &'static str {
        match self {
            Tier::Routine => "routine",
            Tier::Escalated => "escalated",
            Tier::Irreversible => "irreversible",
        }
    }
}

/// An action policy can be asked about. The set is the schema's, and a test keeps them equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Push,
    OpenPr,
    Comment,
    RequestHost,
    Merge,
    ChangePolicy,
    Publish,
    SpendOverCap,
    ExportCredentials,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::Read,
        Action::Push,
        Action::OpenPr,
        Action::Comment,
        Action::RequestHost,
        Action::Merge,
        Action::ChangePolicy,
        Action::Publish,
        Action::SpendOverCap,
        Action::ExportCredentials,
    ];

    /// The action's name in the schema.
    pub fn id(self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Push => "push",
            Action::OpenPr => "open_pr",
            Action::Comment => "comment",
            Action::RequestHost => "request_host",
            Action::Merge => "merge",
            Action::ChangePolicy => "change_policy",
            Action::Publish => "publish",
            Action::SpendOverCap => "spend_over_cap",
            Action::ExportCredentials => "export_credentials",
        }
    }

    /// The tier the schema puts it in. A test compares this with the schema itself.
    pub fn tier(self) -> Tier {
        match self {
            Action::Read | Action::Push | Action::OpenPr | Action::Comment => Tier::Routine,
            Action::RequestHost => Tier::Escalated,
            Action::Merge
            | Action::ChangePolicy
            | Action::Publish
            | Action::SpendOverCap
            | Action::ExportCredentials => Tier::Irreversible,
        }
    }
}

/// What the engine knows about the job from its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFacts {
    pub tainted: bool,
    /// `owner/name`.
    pub repo: String,
}

/// What the job is acting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFacts {
    pub repo: String,
    /// The branch is under the job's branch prefix; the engine compares them.
    pub in_branch_prefix: bool,
    pub protected: bool,
}

/// One question for the policies. Nothing in it is taken from a sandbox (ADR-0008: the proof does not
/// cover how these are filled in, so the engine fills them in from its own state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRequest {
    pub action: Action,
    pub job: JobFacts,
    pub target: TargetFacts,
    /// The operator's signature on an approval for this action was verified.
    pub approved: bool,
    /// The request would use a subscription token.
    pub subscription_token: bool,
}

impl PolicyRequest {
    pub(crate) fn principal(&self) -> &'static str {
        r#"Job::"job""#
    }

    pub(crate) fn resource(&self) -> &'static str {
        r#"Target::"target""#
    }

    pub(crate) fn action_uid(&self) -> String {
        format!(r#"Action::"{}""#, self.action.id())
    }

    pub(crate) fn entities_json(&self) -> String {
        format!(
            r#"[{{"uid":{{"type":"Job","id":"job"}},"attrs":{{"tainted":{},"repo":{}}},"parents":[]}},{{"uid":{{"type":"Target","id":"target"}},"attrs":{{"repo":{},"in_branch_prefix":{},"protected":{}}},"parents":[]}}]"#,
            self.job.tainted,
            json_string(&self.job.repo),
            json_string(&self.target.repo),
            self.target.in_branch_prefix,
            self.target.protected,
        )
    }

    pub(crate) fn context_json(&self) -> String {
        format!(
            r#"{{"approved":{},"subscription_token":{}}}"#,
            self.approved, self.subscription_token
        )
    }
}

/// A JSON string literal for `text`, escaped so that nothing in it can end the string or add fields.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::json_string;

    #[test]
    fn a_string_cannot_end_itself_or_start_a_field() {
        assert_eq!(json_string(r#"a"b"#), r#""a\"b""#);
        assert_eq!(json_string(r"a\b"), r#""a\\b""#);
        assert_eq!(json_string("a\nb\u{0}\u{7f}"), r#""a\u000ab\u0000\u007f""#);
        assert_eq!(json_string("é/😀"), "\"é/😀\"");
    }
}
