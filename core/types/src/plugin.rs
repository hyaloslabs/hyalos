//! What an installed skill or plugin may do (ADR-0005, ADR-0006, ADR-0066, ADR-0085).
//!
//! A [`Grant`] is the capabilities a human granted a plugin. Its text ([`crate::PluginText`]) carries
//! it, so a prompt that holds plugin text also holds the grant, and the job is bounded by both: what an
//! actor steered by a plugin may do is the intersection of the plugin's grant and the job's manifest.
//! A grant can only narrow a manifest, never widen it. It is written in the capability vocabulary
//! ([`crate::capability`]), which the manifest shares.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};

use crate::Capabilities;

/// The capabilities granted to one plugin. A local install starts with none (ADR-0006).
pub type Grant = Capabilities;

/// The longest name a plugin may have, as the Agent Skills format allows.
pub const MAX_PLUGIN_NAME_LEN: usize = 64;

/// An installed plugin's name (ADR-0089): 1 to 64 lowercase letters, digits and hyphens, not starting
/// or ending with a hyphen and with no two in a row, as the Agent Skills format names a skill. The name
/// comes from the plugin's own `SKILL.md`, so it is plugin-authored, but a name of this syntax can hold
/// nothing but those characters, so it may be printed, stored and used as a directory name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginName(String);

impl PluginName {
    pub fn new(name: &str) -> Option<Self> {
        let valid = !name.is_empty()
            && name.len() <= MAX_PLUGIN_NAME_LEN
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--");
        valid.then(|| Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PluginName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for PluginName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PluginName({})", self.0)
    }
}

impl Serialize for PluginName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PluginName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        PluginName::new(&text).ok_or_else(|| {
            D::Error::custom(
                "a plugin name is 1 to 64 lowercase letters, digits and single hyphens",
            )
        })
    }
}

/// Where an installed plugin came from (ADR-0006, ADR-0089, ADR-0090).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    /// Read from a directory on the operator's machine. It has no signature or transparency-log
    /// entry, so every capability it holds was granted by a human (ADR-0006).
    Local,
    /// Resolved by name in a plugin index (ADR-0090): the index it was found in and the version it
    /// named. Its files had the digest the index gave.
    Index {
        location: IndexLocation,
        version: PluginVersion,
    },
}

/// How old a trusted-root snapshot may be, by default, before installs are refused (ADR-0007).
pub const DEFAULT_MAX_SNAPSHOT_AGE_DAYS: u32 = 90;
/// How old an install bundle's revocation list may be, by default (ADR-0007).
pub const DEFAULT_MAX_REVOCATION_AGE_DAYS: u32 = 30;
/// The longest maximum age an install policy may set.
pub const MAX_AGE_DAYS: u32 = 3650;
/// The most index locations an install policy may trust.
pub const MAX_POLICY_INDEXES: usize = 16;

/// What the operator lets install without them (ADR-0007, ADR-0091): a plugin from one of
/// `indexes`, asking for nothing beyond `ceiling`. Anything else is the operator's decision each
/// time. It also holds the maximum ages of ADR-0007, which only a new policy changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallPolicy {
    pub ceiling: Capabilities,
    pub indexes: BTreeSet<IndexLocation>,
    pub max_snapshot_age_days: u32,
    pub max_revocation_age_days: u32,
}

impl InstallPolicy {
    /// Whether the ceiling is in the vocabulary, the ages are 1 to [`MAX_AGE_DAYS`], and there are
    /// at most [`MAX_POLICY_INDEXES`] indexes.
    pub fn is_valid(&self) -> bool {
        let age = 1..=MAX_AGE_DAYS;
        self.ceiling.check().is_ok()
            && age.contains(&self.max_snapshot_age_days)
            && age.contains(&self.max_revocation_age_days)
            && self.indexes.len() <= MAX_POLICY_INDEXES
    }

    /// Whether a plugin from `source` granted `grant` may install under this policy alone: it came
    /// from an index the policy trusts, and asks for nothing the ceiling does not hold. A local
    /// install never does.
    pub fn covers(&self, source: &PluginSource, grant: &Grant) -> bool {
        match source {
            PluginSource::Local => false,
            PluginSource::Index { location, .. } => {
                self.indexes.contains(location) && !self.ceiling.diff(grant).widens()
            }
        }
    }
}

/// The longest version a plugin index may give.
pub const MAX_PLUGIN_VERSION_LEN: usize = 64;
/// The longest location of a plugin index.
pub const MAX_INDEX_LOCATION_LEN: usize = 1024;

/// A plugin's version as its index gives it (ADR-0090): 1 to 64 ASCII letters, digits, `.`, `+` and
/// `-`, starting with a letter or digit. It is compared only for equality; the index's order says
/// which is the latest.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginVersion(String);

impl PluginVersion {
    pub fn new(text: &str) -> Option<Self> {
        let valid = (1..=MAX_PLUGIN_VERSION_LEN).contains(&text.len())
            && text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'));
        valid.then(|| Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a plugin index is (ADR-0090): an `https://` URL, an `http://` URL on loopback, or an
/// absolute path to a directory (an offline bundle). Up to 1,024 characters with no space or control
/// character. The operator gives it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndexLocation(String);

impl IndexLocation {
    pub fn new(text: &str) -> Option<Self> {
        let loopback = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
            .iter()
            .any(|base| {
                text.strip_prefix(base)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
            });
        let valid = (1..=MAX_INDEX_LOCATION_LEN).contains(&text.len())
            && !text.bytes().any(|b| b.is_ascii_control() || b == b' ')
            && (text.starts_with("https://") || loopback || text.starts_with('/'));
        valid.then(|| Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it is a directory on this machine rather than a URL.
    pub fn is_dir(&self) -> bool {
        self.0.starts_with('/')
    }
}

macro_rules! text_type {
    ($name:ident, $what:literal) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                $name::new(&text).ok_or_else(|| D::Error::custom($what))
            }
        }
    };
}

text_type!(PluginVersion, "not a plugin version");
text_type!(IndexLocation, "not a plugin index location");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp};
    use std::collections::BTreeSet;

    fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
        items.map(String::from).into()
    }

    fn manifest() -> Manifest {
        Manifest {
            version: MANIFEST_VERSION,
            job: JobId::from_seq(2),
            role: Role::Actor,
            tainted: true,
            repo: "octo/repo".into(),
            branch_prefix: "hyalos/job-2".into(),
            pr_base: Some("main".into()),
            actions: set(["read", "push", "open_pr"]),
            egress_hosts: set(["docs.example"]),
            tools: BTreeSet::new(),
            data_flows: set(["job->model", "job->forge", "job->web"]),
            budget: Budget { tokens: 100 },
            deadline: Timestamp::from_millis(9),
            approval: None,
            handles: BTreeSet::new(),
            result: None,
            closes: None,
        }
    }

    #[test]
    fn a_version_and_an_index_location_are_checked_text() {
        for good in ["1", "1.2.0", "2.0.0-rc.1+build.5", &"9".repeat(64)] {
            assert!(PluginVersion::new(good).is_some(), "{good}");
        }
        for bad in ["", ".1", "1 2", "1/2", "v1\n", &"9".repeat(65)] {
            assert!(PluginVersion::new(bad).is_none(), "{bad:?}");
        }
        for good in [
            "https://github.com/o/r/releases/latest/download",
            "http://localhost:8080/index",
            "http://127.0.0.1",
            "/srv/bundle",
        ] {
            assert!(IndexLocation::new(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "http://example.com",
            "http://localhost.evil.com",
            "ftp://x",
            "bundle",
            "https://a b",
            "https://x\n",
            &format!("https://{}", "a".repeat(1020)),
        ] {
            assert!(IndexLocation::new(bad).is_none(), "{bad:?}");
        }
        let source = PluginSource::Index {
            location: IndexLocation::new("/srv/bundle").unwrap(),
            version: PluginVersion::new("1.2.0").unwrap(),
        };
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(
            json,
            r#"{"index":{"location":"/srv/bundle","version":"1.2.0"}}"#
        );
        assert_eq!(serde_json::from_str::<PluginSource>(&json).unwrap(), source);
        assert_eq!(
            serde_json::to_string(&PluginSource::Local).unwrap(),
            r#""local""#
        );
        assert!(
            serde_json::from_str::<PluginSource>(
                r#"{"index":{"location":"http://x.com","version":"1"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_plugin_name_is_the_agent_skills_syntax() {
        for good in ["pdf", "pdf-tools", "a1", &"a".repeat(64)] {
            assert!(PluginName::new(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "PDF",
            "-pdf",
            "pdf-",
            "pdf--tools",
            "pdf_tools",
            "pdf.tools",
            "../x",
            "a b",
            "p\u{1b}",
            &"a".repeat(65),
        ] {
            assert!(PluginName::new(bad).is_none(), "{bad}");
        }
        let name = PluginName::new("pdf-tools").unwrap();
        assert_eq!(serde_json::to_string(&name).unwrap(), r#""pdf-tools""#);
        assert!(serde_json::from_str::<PluginName>(r#""../etc""#).is_err());
        assert_eq!(
            serde_json::to_string(&PluginSource::Local).unwrap(),
            r#""local""#
        );
    }

    #[test]
    fn a_grant_broader_than_the_manifest_leaves_only_the_intersection() {
        let broad = Grant {
            actions: set(["read", "push", "publish", "change_policy"]),
            egress_hosts: set(["docs.example", "exfil.example"]),
            tools: set(["shell"]),
            data_flows: set(["job->model", "job->forge", "job->web"]),
        };
        let bounded = broad.bound(&manifest());
        assert_eq!(bounded.actions, set(["read", "push"]));
        assert_eq!(bounded.egress_hosts, set(["docs.example"]));
        assert!(bounded.tools.is_empty());
        assert_eq!(bounded.data_flows, manifest().data_flows);
        // Nothing else changes, and the result is something the grant allows.
        assert_eq!(bounded.repo, "octo/repo");
        assert_eq!(bounded.budget, manifest().budget);
        assert!(broad.allows(&bounded));
        assert!(!broad.allows(&manifest()));
    }

    #[test]
    fn a_grant_never_widens_a_manifest() {
        let wide = Grant {
            actions: set(["read", "push", "open_pr", "publish"]),
            egress_hosts: set(["docs.example", "exfil.example"]),
            tools: set(["shell"]),
            data_flows: set(["job->model", "job->forge", "job->web"]),
        };
        let narrow = Manifest {
            actions: set(["read"]),
            egress_hosts: BTreeSet::new(),
            data_flows: set(["job->model"]),
            ..manifest()
        };
        assert_eq!(wide.bound(&narrow), narrow);
        // No grant at all leaves no capability.
        let none = Grant::none().bound(&manifest());
        assert!(
            none.actions.is_empty() && none.egress_hosts.is_empty() && none.data_flows.is_empty()
        );
        assert!(Grant::none().allows(&none));
    }

    #[test]
    fn a_grant_reads_back_with_no_unknown_field() {
        let read: Grant = serde_json::from_str(r#"{"actions":["read"]}"#).unwrap();
        assert_eq!(read.actions, set(["read"]));
        assert!(serde_json::from_str::<Grant>(r#"{"actions":[],"budget":5}"#).is_err());
    }
}
