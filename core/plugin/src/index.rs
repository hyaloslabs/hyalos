//! A plugin index (ADR-0090): what a distribution source says it holds, by name.
//!
//! An index is `index.json` beside the files it names, as a release's assets or as a directory (the
//! offline bundle). For each plugin name it lists the versions published, oldest first, each with the
//! digest of its archive, the archive's file name and the file name of its Sigstore bundle. Install
//! resolves a name here, fetches the archive beside the index, and refuses it unless its SHA-256 is
//! the digest given. The index is read within limits, and an error names nothing it holds.

use std::collections::{BTreeMap, BTreeSet};

use hyalos_types::{Digest, PluginName, PluginVersion, ReasonCode};
use serde::Deserialize;

use crate::skill::plain;

/// The index's file name, at the source's root.
pub const INDEX_FILE: &str = "index.json";
/// The version of the index format this build reads.
pub const INDEX_VERSION: u32 = 1;
/// The most bytes an index may hold.
pub const MAX_INDEX: u64 = 1 << 20;

/// One published version of a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: PluginName,
    pub version: PluginVersion,
    /// The SHA-256 of the archive, which is the skill's digest.
    pub digest: Digest,
    /// The archive's file name, beside the index.
    pub artifact: String,
    /// The Sigstore bundle's file name, beside the index. Verifying it is S7.12's (ADR-0007).
    pub bundle: String,
}

/// An index as read: each name's versions, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    plugins: BTreeMap<PluginName, Vec<Entry>>,
}

/// Why an index cannot be read or does not hold what was asked for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    /// Larger than an index may be.
    #[error("the index is larger than {MAX_INDEX} bytes")]
    TooLarge,
    /// Not an index document: not JSON, an unknown field, or a name, version, digest or file name
    /// that is not well formed.
    #[error("the index is not an index document this build reads")]
    Malformed,
    /// A version of the format this build does not read.
    #[error("the index is version {0}; this build reads version {INDEX_VERSION}")]
    Version(u32),
    /// The same version twice for one name, or the same file named twice.
    #[error("the index lists a version or a file twice")]
    Duplicate,
    /// No plugin of that name.
    #[error("the index has no plugin of that name")]
    NoSuchPlugin,
    /// The plugin has no such version.
    #[error("the index has no such version of that plugin")]
    NoSuchVersion,
}

impl IndexError {
    pub fn code(&self) -> ReasonCode {
        let code = match self {
            Self::TooLarge => "index.too_large",
            Self::Malformed => "index.malformed",
            Self::Version(_) => "index.version",
            Self::Duplicate => "index.duplicate",
            Self::NoSuchPlugin => "index.no_such_plugin",
            Self::NoSuchVersion => "index.no_such_version",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    plugins: BTreeMap<String, Vec<EntryDocument>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryDocument {
    version: String,
    digest: String,
    artifact: String,
    bundle: String,
}

impl Index {
    /// Reads `bytes` as an index document.
    pub fn parse(bytes: &[u8]) -> Result<Index, IndexError> {
        if bytes.len() as u64 > MAX_INDEX {
            return Err(IndexError::TooLarge);
        }
        // The version first, so a newer format is named as such rather than as malformed.
        #[derive(Deserialize)]
        struct Versioned {
            version: u32,
        }
        let versioned: Versioned =
            serde_json::from_slice(bytes).map_err(|_| IndexError::Malformed)?;
        if versioned.version != INDEX_VERSION {
            return Err(IndexError::Version(versioned.version));
        }
        let document: Document =
            serde_json::from_slice(bytes).map_err(|_| IndexError::Malformed)?;
        debug_assert_eq!(document.version, INDEX_VERSION);
        let mut files = BTreeSet::new();
        let mut plugins = BTreeMap::new();
        for (name, versions) in document.plugins {
            let name = PluginName::new(&name).ok_or(IndexError::Malformed)?;
            let mut entries: Vec<Entry> = Vec::new();
            for doc in versions {
                let entry = Entry {
                    name: name.clone(),
                    version: PluginVersion::new(&doc.version).ok_or(IndexError::Malformed)?,
                    digest: Digest::parse(&doc.digest).ok_or(IndexError::Malformed)?,
                    artifact: Some(doc.artifact)
                        .filter(|a| plain(a))
                        .ok_or(IndexError::Malformed)?,
                    bundle: Some(doc.bundle)
                        .filter(|b| plain(b))
                        .ok_or(IndexError::Malformed)?,
                };
                if entries.iter().any(|e| e.version == entry.version)
                    || entry.artifact == entry.bundle
                    || !files.insert(entry.artifact.clone())
                    || !files.insert(entry.bundle.clone())
                {
                    return Err(IndexError::Duplicate);
                }
                entries.push(entry);
            }
            if entries.is_empty() {
                return Err(IndexError::Malformed);
            }
            plugins.insert(name, entries);
        }
        if files.contains(INDEX_FILE) {
            return Err(IndexError::Duplicate);
        }
        Ok(Index { plugins })
    }

    /// The entry for `name`: `version` if given, otherwise the latest, which is the last listed.
    pub fn resolve(
        &self,
        name: &PluginName,
        version: Option<&PluginVersion>,
    ) -> Result<&Entry, IndexError> {
        let entries = self.plugins.get(name).ok_or(IndexError::NoSuchPlugin)?;
        match version {
            Some(v) => entries
                .iter()
                .find(|e| e.version == *v)
                .ok_or(IndexError::NoSuchVersion),
            None => entries.last().ok_or(IndexError::NoSuchPlugin),
        }
    }

    /// Every name the index holds, with its versions oldest first.
    pub fn plugins(&self) -> &BTreeMap<PluginName, Vec<Entry>> {
        &self.plugins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn doc(entries: &str) -> String {
        format!(r#"{{"version":1,"plugins":{{{entries}}}}}"#)
    }

    fn entry(version: &str, byte: u8) -> String {
        format!(
            r#"{{"version":"{version}","digest":"{}","artifact":"pdf-{version}.skill","bundle":"pdf-{version}.sigstore.json"}}"#,
            hex(byte)
        )
    }

    fn name(text: &str) -> PluginName {
        PluginName::new(text).unwrap()
    }

    #[test]
    fn a_name_resolves_to_its_latest_version_or_the_one_asked_for() {
        let text = doc(&format!(
            r#""pdf-tools":[{},{}]"#,
            entry("1.0.0", 1),
            entry("1.1.0", 2)
        ));
        let index = Index::parse(text.as_bytes()).unwrap();
        let latest = index.resolve(&name("pdf-tools"), None).unwrap();
        assert_eq!(latest.version.as_str(), "1.1.0");
        assert_eq!(latest.digest, Digest::from_bytes([2; 32]));
        assert_eq!(latest.artifact, "pdf-1.1.0.skill");
        assert_eq!(latest.bundle, "pdf-1.1.0.sigstore.json");
        let old = PluginVersion::new("1.0.0").unwrap();
        assert_eq!(
            index
                .resolve(&name("pdf-tools"), Some(&old))
                .unwrap()
                .digest,
            Digest::from_bytes([1; 32])
        );
        let missing = PluginVersion::new("9").unwrap();
        assert_eq!(
            index.resolve(&name("pdf-tools"), Some(&missing)),
            Err(IndexError::NoSuchVersion)
        );
        assert_eq!(
            index.resolve(&name("lint"), None),
            Err(IndexError::NoSuchPlugin)
        );
    }

    #[test]
    fn a_malformed_index_is_refused() {
        let one = entry("1.0.0", 1);
        for (text, want) in [
            ("not json".to_string(), IndexError::Malformed),
            (
                r#"{"version":2,"plugins":{}}"#.to_string(),
                IndexError::Version(2),
            ),
            (doc(r#""PDF":[]"#), IndexError::Malformed),
            (doc(r#""pdf":[]"#), IndexError::Malformed),
            (
                doc(&format!(r#""pdf":[{one},{one}]"#)),
                IndexError::Duplicate,
            ),
            (
                doc(&format!(r#""pdf":[{one}],"lint":[{one}]"#)),
                IndexError::Duplicate,
            ),
            (
                doc(&one.replace("pdf-1.0.0.skill", "../etc/passwd")),
                IndexError::Malformed,
            ),
            (
                doc(&format!(
                    r#""pdf":[{}]"#,
                    one.replace("pdf-1.0.0.skill", "index.json")
                )),
                IndexError::Duplicate,
            ),
            (
                doc(&format!(r#""pdf":[{}]"#, one.replace(&hex(1), "zz"))),
                IndexError::Malformed,
            ),
            (
                doc(&format!(
                    r#""pdf":[{}]"#,
                    one.replace("}", r#","extra":1}"#)
                )),
                IndexError::Malformed,
            ),
        ] {
            assert_eq!(Index::parse(text.as_bytes()), Err(want), "{text}");
        }
        let big = vec![b' '; MAX_INDEX as usize + 1];
        assert_eq!(Index::parse(&big), Err(IndexError::TooLarge));
        // No error names what the index holds.
        assert!(!IndexError::Malformed.to_string().contains("pdf"));
    }
}
