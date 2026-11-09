//! The capability vocabulary (ADR-0085): one versioned schema for what a job may do, shared by the
//! job's manifest and by what a plugin asks for or is granted (ADR-0004, ADR-0006).
//!
//! A capability is a name in one of four fields. Each field is asked of the policy as one action
//! ([`Field::asked_as`]), and each has a closed set of names or a syntax its names must have
//! ([`Capabilities::check`]). The manifest holds the same four sets ([`Manifest::capabilities`]), so
//! a plugin's capabilities, the grant a human gave it, the job it narrows and the diff an operator
//! reads are all in these terms. A new field, or a change to what a field accepts, bumps
//! [`CAPABILITY_VERSION`] and needs an ADR: a test pins both.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Manifest;
use crate::manifest::flow;

/// The version of the vocabulary a plugin's capabilities are written in.
pub const CAPABILITY_VERSION: u32 = 1;

/// The most bytes a capabilities document may hold. A larger one is refused unread.
pub const MAX_CAPABILITY_DOCUMENT: usize = 64 * 1024;

/// The actions a job may be given on its repository. Each is asked of the policy by its own name.
pub mod action {
    /// Clone and fetch, and read issues and pull requests.
    pub const READ: &str = "read";
    /// Push to the job's own branches.
    pub const PUSH: &str = "push";
    pub const OPEN_PR: &str = "open_pr";
    pub const COMMENT: &str = "comment";
    /// Merge a pull request, only with the operator's approval bound to its head (ADR-0071).
    pub const MERGE: &str = "merge";
    /// The rest the policy can be asked about. The shipped policy never permits them to a job.
    pub const CHANGE_POLICY: &str = "change_policy";
    pub const PUBLISH: &str = "publish";
    pub const SPEND_OVER_CAP: &str = "spend_over_cap";
    pub const EXPORT_CREDENTIALS: &str = "export_credentials";
    /// Every action there is.
    pub const ALL: [&str; 9] = [
        READ,
        PUSH,
        OPEN_PR,
        COMMENT,
        MERGE,
        CHANGE_POLICY,
        PUBLISH,
        SPEND_OVER_CAP,
        EXPORT_CREDENTIALS,
    ];

    /// Whether `name` is an action this build knows.
    pub fn known(name: &str) -> bool {
        ALL.contains(&name)
    }
}

/// The fields of the vocabulary, in the order they are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Actions,
    EgressHosts,
    Tools,
    DataFlows,
}

impl Field {
    pub const ALL: [Field; 4] = [
        Field::Actions,
        Field::EgressHosts,
        Field::Tools,
        Field::DataFlows,
    ];

    /// The field's name, in the manifest and in a plugin's capabilities alike.
    pub fn name(self) -> &'static str {
        match self {
            Field::Actions => "actions",
            Field::EgressHosts => "egress_hosts",
            Field::Tools => "tools",
            Field::DataFlows => "data_flows",
        }
    }

    /// The policy action a name in this field is asked as. An action is asked by its own name.
    pub fn asked_as(self) -> Option<&'static str> {
        match self {
            Field::Actions => None,
            Field::EgressHosts => Some("request_host"),
            Field::Tools => Some("use_tool"),
            Field::DataFlows => Some("data_flow"),
        }
    }

    /// Whether `name` may be in this field.
    pub fn accepts(self, name: &str) -> bool {
        match self {
            Field::Actions => action::known(name),
            Field::EgressHosts => is_host(name),
            Field::Tools => is_tool(name),
            Field::DataFlows => flow::known(name),
        }
    }
}

/// A host name as the broker's egress gate compares it: lower case, dot-separated labels of letters,
/// digits and inner hyphens, with no trailing dot. No port, no wildcard, and no address, including the
/// forms a resolver reads as numbers (a numeric last label, or `0x...`): egress is granted host by host.
fn is_host(name: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or_default();
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
        && !last.bytes().all(|b| b.is_ascii_digit())
        && !last.starts_with("0x")
}

/// A tool's name as MCP allows it: 1 to 128 ASCII letters, digits, `_`, `-` and `.`.
fn is_tool(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// A set of capabilities: what a plugin asks for, what a human granted it (a [`crate::Grant`]), or
/// a manifest's. A plugin installed locally starts with none (ADR-0006).
///
/// Deserializing one checks no name. Anything read from outside, such as a plugin's capabilities at
/// install, is read with [`Capabilities::from_document`], which does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(default)]
    pub actions: BTreeSet<String>,
    #[serde(default)]
    pub egress_hosts: BTreeSet<String>,
    #[serde(default)]
    pub tools: BTreeSet<String>,
    #[serde(default)]
    pub data_flows: BTreeSet<String>,
}

/// Why a plugin's capabilities cannot be used. Names a field and nothing the document said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityError {
    /// Not a capabilities document: not JSON, a field this version does not have, or a wrong type.
    Malformed,
    /// Written in a version this build does not read.
    Version(u32),
    /// A name the field does not accept.
    Unknown(Field),
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => write!(f, "not a capabilities document"),
            Self::Version(v) => write!(
                f,
                "capabilities version {v}; this build reads version {CAPABILITY_VERSION}"
            ),
            Self::Unknown(field) => write!(f, "a name {} does not accept", field.name()),
        }
    }
}

/// A plugin's capabilities as written: the version, then the fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    #[serde(default)]
    actions: BTreeSet<String>,
    #[serde(default)]
    egress_hosts: BTreeSet<String>,
    #[serde(default)]
    tools: BTreeSet<String>,
    #[serde(default)]
    data_flows: BTreeSet<String>,
}

impl Capabilities {
    /// No capabilities: what a plugin has before a human grants it any.
    pub fn none() -> Self {
        Self::default()
    }

    /// Reads a plugin's capabilities, written as `{"version": 1, "actions": [...], ...}`. A version
    /// this build does not read, a field it does not know, or a name a field does not accept is
    /// refused, never skipped.
    pub fn from_document(bytes: &[u8]) -> Result<Self, CapabilityError> {
        if bytes.len() > MAX_CAPABILITY_DOCUMENT {
            return Err(CapabilityError::Malformed);
        }
        let document: Document =
            serde_json::from_slice(bytes).map_err(|_| CapabilityError::Malformed)?;
        if document.version != CAPABILITY_VERSION {
            return Err(CapabilityError::Version(document.version));
        }
        let capabilities = Self {
            actions: document.actions,
            egress_hosts: document.egress_hosts,
            tools: document.tools,
            data_flows: document.data_flows,
        };
        capabilities.check()?;
        Ok(capabilities)
    }

    /// The names in `field`.
    pub fn field(&self, field: Field) -> &BTreeSet<String> {
        match field {
            Field::Actions => &self.actions,
            Field::EgressHosts => &self.egress_hosts,
            Field::Tools => &self.tools,
            Field::DataFlows => &self.data_flows,
        }
    }

    fn field_mut(&mut self, field: Field) -> &mut BTreeSet<String> {
        match field {
            Field::Actions => &mut self.actions,
            Field::EgressHosts => &mut self.egress_hosts,
            Field::Tools => &mut self.tools,
            Field::DataFlows => &mut self.data_flows,
        }
    }

    /// Every name is one its field accepts. The first field that fails is named.
    pub fn check(&self) -> Result<(), CapabilityError> {
        for field in Field::ALL {
            if !self.field(field).iter().all(|name| field.accepts(name)) {
                return Err(CapabilityError::Unknown(field));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        Field::ALL.iter().all(|f| self.field(*f).is_empty())
    }

    /// Whether every capability here is also in `other`.
    pub fn within(&self, other: &Capabilities) -> bool {
        Field::ALL
            .iter()
            .all(|f| self.field(*f).is_subset(other.field(*f)))
    }

    /// `manifest` with each capability cut to what these capabilities also hold. Everything else is
    /// kept, so the result is never wider than `manifest`.
    pub fn bound(&self, manifest: &Manifest) -> Manifest {
        let has = manifest.capabilities();
        let cut = |field: Field| -> BTreeSet<String> {
            has.field(field)
                .intersection(self.field(field))
                .cloned()
                .collect()
        };
        Manifest {
            actions: cut(Field::Actions),
            egress_hosts: cut(Field::EgressHosts),
            tools: cut(Field::Tools),
            data_flows: cut(Field::DataFlows),
            ..manifest.clone()
        }
    }

    /// Whether every capability `manifest` holds is in these.
    pub fn allows(&self, manifest: &Manifest) -> bool {
        manifest.capabilities().within(self)
    }

    /// What `newer` adds to these capabilities, what it takes away and what it keeps, field by
    /// field: what an operator reads before an install or an update (ADR-0088). A first install is
    /// the diff from [`Capabilities::none`].
    pub fn diff(&self, newer: &Capabilities) -> Diff {
        let mut diff = Diff::default();
        for field in Field::ALL {
            let (old, new) = (self.field(field), newer.field(field));
            *diff.added.field_mut(field) = new.difference(old).cloned().collect();
            *diff.removed.field_mut(field) = old.difference(new).cloned().collect();
            *diff.unchanged.field_mut(field) = old.intersection(new).cloned().collect();
        }
        diff
    }
}

/// How one set of capabilities differs from another. Every name of either set is in exactly one of
/// the three.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Capabilities,
    pub removed: Capabilities,
    pub unchanged: Capabilities,
}

impl Diff {
    /// Whether the newer set asks for anything the older did not: what needs consent (ADR-0007).
    pub fn widens(&self) -> bool {
        !self.added.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
        items.map(String::from).into()
    }

    #[test]
    fn each_field_accepts_only_its_own_names() {
        for good in ["docs.rs", "api.github.com", "localhost", "a-b.example"] {
            assert!(Field::EgressHosts.accepts(good), "{good}");
        }
        for bad in [
            "",
            "Docs.rs",
            "docs.rs:443",
            "*.example",
            "-a.example",
            "a..example",
            "a b",
            "https://x",
            "docs.rs.",
            "127.0.0.1",
            "0x7f000001",
            "::1",
        ] {
            assert!(!Field::EgressHosts.accepts(bad), "{bad}");
        }
        assert!(Field::Tools.accepts("search_issues") && Field::Tools.accepts("fs.read-1"));
        for bad in ["", "a b", "a/b", &"x".repeat(129)] {
            assert!(!Field::Tools.accepts(bad), "{bad}");
        }
        assert!(Field::Actions.accepts("open_pr") && !Field::Actions.accepts("use_tool"));
        assert!(Field::DataFlows.accepts(flow::WEB) && !Field::DataFlows.accepts("job->disk"));
    }

    #[test]
    fn a_document_is_read_only_whole_and_in_this_version() {
        let read = Capabilities::from_document(
            br#"{"version":1,"actions":["read"],"egress_hosts":["docs.rs"]}"#,
        )
        .unwrap();
        assert_eq!(read.actions, set(["read"]));
        assert_eq!(read.egress_hosts, set(["docs.rs"]));
        assert!(read.tools.is_empty() && read.data_flows.is_empty());
        for (bytes, want) in [
            (&br#"{"actions":["read"]}"#[..], CapabilityError::Malformed),
            (br#"{"version":1,"budget":5}"#, CapabilityError::Malformed),
            (br#"{"version":2}"#, CapabilityError::Version(2)),
            (
                br#"{"version":1,"actions":["read","use_tool"]}"#,
                CapabilityError::Unknown(Field::Actions),
            ),
            (
                br#"{"version":1,"egress_hosts":["*.example"]}"#,
                CapabilityError::Unknown(Field::EgressHosts),
            ),
            (
                br#"{"version":1,"data_flows":["job->disk"]}"#,
                CapabilityError::Unknown(Field::DataFlows),
            ),
        ] {
            assert_eq!(Capabilities::from_document(bytes), Err(want));
        }
        let big = format!(
            r#"{{"version":1,"tools":["{}"]}}"#,
            " ".repeat(MAX_CAPABILITY_DOCUMENT)
        );
        assert_eq!(
            Capabilities::from_document(big.as_bytes()),
            Err(CapabilityError::Malformed)
        );
        // An error names the field, never the name it refused.
        let shown = CapabilityError::Unknown(Field::EgressHosts).to_string();
        assert!(shown.contains("egress_hosts") && !shown.contains("example"));
    }

    #[test]
    fn the_diff_says_what_an_update_adds_and_takes_away() {
        let old = Capabilities {
            actions: set(["read", "push"]),
            egress_hosts: set(["docs.rs"]),
            ..Capabilities::none()
        };
        let new = Capabilities {
            actions: set(["read", "open_pr"]),
            egress_hosts: set(["docs.rs"]),
            tools: set(["search"]),
            ..Capabilities::none()
        };
        let diff = old.diff(&new);
        assert_eq!(diff.added.actions, set(["open_pr"]));
        assert_eq!(diff.added.tools, set(["search"]));
        assert_eq!(diff.removed.actions, set(["push"]));
        assert!(diff.added.egress_hosts.is_empty() && diff.removed.egress_hosts.is_empty());
        assert_eq!(diff.unchanged.actions, set(["read"]));
        assert_eq!(diff.unchanged.egress_hosts, set(["docs.rs"]));
        assert!(diff.unchanged.tools.is_empty() && diff.unchanged.data_flows.is_empty());
        assert!(diff.widens());
        // Taking capabilities away, or changing nothing, needs no consent.
        let same = old.diff(&old);
        assert!(!same.widens() && same.added.is_empty() && same.removed.is_empty());
        assert_eq!(same.unchanged, old);
        let narrower = Capabilities {
            actions: set(["read"]),
            ..Capabilities::none()
        };
        assert!(!old.diff(&narrower).widens());
        // A first install is the diff from nothing: everything is added.
        let first = Capabilities::none().diff(&new);
        assert_eq!(first.added, new);
        assert!(first.removed.is_empty() && first.unchanged.is_empty());
    }
}
