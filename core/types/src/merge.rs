//! What a merge approval names (ADR-0012, ADR-0071): the repository, the pull request, the head commit
//! the operator reviewed and how it is merged. Each is checked as it is made and as it is read back,
//! so an event or a manifest cannot carry a malformed one.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A git commit id: 40 lowercase hex digits (SHA-1), or 64 (SHA-256).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommitSha(String);

impl CommitSha {
    /// `text` as a commit id, or `None` if it is not exactly one. Upper case is refused rather than
    /// folded, so the id approved is the id written.
    pub fn new(text: &str) -> Option<Self> {
        let ok = matches!(text.len(), 40 | 64)
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        ok.then(|| Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CommitSha {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Self::new(&text).ok_or_else(|| "a commit id is 40 or 64 lowercase hex digits".to_string())
    }
}

impl From<CommitSha> for String {
    fn from(sha: CommitSha) -> String {
        sha.0
    }
}

impl fmt::Debug for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CommitSha({})", self.0)
    }
}

impl fmt::Display for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a pull request is merged, in GitHub's names. Part of what the operator signs, so the broker
/// cannot merge another way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const ALL: [MergeMethod; 3] =
        [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase];

    /// The name GitHub's `merge_method` takes, which is also this method's name here.
    pub fn as_str(self) -> &'static str {
        match self {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == name)
    }
}

/// The largest pull request number an approval may name.
pub const MAX_PR: u64 = i32::MAX as u64;

/// Whether `repo` is `owner/name` as GitHub spells them: an owner of 1 to 39 letters, digits and
/// hyphens, and a name of 1 to 100 letters, digits, `.`, `_` and `-` that is not `.` or `..`.
pub fn valid_repo(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let name_ok = (1..=100).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    owner_ok && name_ok
}

/// Whether `pr` is a pull request number an approval may name.
pub fn valid_pr(pr: u64) -> bool {
    (1..=MAX_PR).contains(&pr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_is_exactly_forty_or_sixty_four_lowercase_hex_digits() {
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            CommitSha::new(sha1).map(|s| s.to_string()),
            Some(sha1.into())
        );
        assert!(CommitSha::new(&"a".repeat(64)).is_some());
        for bad in [
            "",
            &sha1[..39],
            &format!("{sha1}0"),
            &sha1.to_uppercase(),
            &sha1.replace('0', "g"),
        ] {
            assert!(CommitSha::new(bad).is_none(), "{bad}");
        }
        assert!(serde_json::from_str::<CommitSha>(r#""ABC""#).is_err());
        assert_eq!(
            serde_json::to_string(&CommitSha::new(sha1).unwrap()).unwrap(),
            format!("\"{sha1}\"")
        );
    }

    #[test]
    fn repos_methods_and_numbers_are_checked() {
        assert!(valid_repo("octo/repo.js"));
        for bad in ["octo", "octo/", "/repo", "o_o/repo", "octo/..", "a/b/c"] {
            assert!(!valid_repo(bad), "{bad}");
        }
        assert!(!valid_repo(&format!("{}/r", "a".repeat(40))));
        assert_eq!(MergeMethod::from_name("squash"), Some(MergeMethod::Squash));
        assert_eq!(MergeMethod::from_name("fast-forward"), None);
        assert!(valid_pr(1) && valid_pr(MAX_PR) && !valid_pr(0) && !valid_pr(MAX_PR + 1));
    }
}
