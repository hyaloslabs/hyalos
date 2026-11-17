//! Who wrote a held issue, as their permission on its repository only (ADR-0103).
//!
//! The broker reads the issue's author, every editor of its body and everyone who renamed it, and asks
//! the forge each one's permission. No login reaches the engine: a writer is a [`Permission`], so the
//! engine and the policy decide on typed facts alone.

use serde::{Deserialize, Serialize};

/// The most writers besides the author a held issue may carry. The broker reports more than it can
/// check as one [`Permission::Unknown`], so the list never says more than was checked.
pub const MAX_EDITORS: usize = 8;

/// A writer's permission on the repository, as the forge's base role says it: `maintain` is `write`
/// and `triage` is `read`. `Unknown` is anything the broker could not establish, and never writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Admin,
    Write,
    Read,
    None,
    Unknown,
}

impl Permission {
    /// The forge's base role, read exactly; anything else is unknown.
    pub fn from_role(role: &str) -> Self {
        match role {
            "admin" => Permission::Admin,
            "write" => Permission::Write,
            "read" => Permission::Read,
            "none" => Permission::None,
            _ => Permission::Unknown,
        }
    }

    /// Whether this writer can push to the repository.
    pub fn can_write(self) -> bool {
        matches!(self, Permission::Admin | Permission::Write)
    }
}

/// Who wrote a held issue: its author's permission, and each other writer's, once each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Writers {
    pub author: Permission,
    #[serde(default)]
    pub editors: Vec<Permission>,
}

impl Writers {
    /// Nothing established: what a read that cannot say who wrote the issue reports.
    pub fn unknown() -> Self {
        Self {
            author: Permission::Unknown,
            editors: Vec::new(),
        }
    }

    /// Everyone who wrote the issue can write to the repository, and the list is one the broker
    /// could have sent: the stakes at which policy may release it (ADR-0025, ADR-0103).
    pub fn all_can_write(&self) -> bool {
        self.editors.len() <= MAX_EDITORS
            && self.author.can_write()
            && self.editors.iter().all(|p| p.can_write())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_writers_who_can_all_push_are_writers_only() {
        let all = |author, editors: &[Permission]| {
            Writers {
                author,
                editors: editors.to_vec(),
            }
            .all_can_write()
        };
        assert!(all(Permission::Write, &[]));
        assert!(all(Permission::Admin, &[Permission::Write]));
        assert!(!all(Permission::Write, &[Permission::Read]));
        assert!(!all(Permission::None, &[]));
        assert!(!all(Permission::Write, &[Permission::Unknown]));
        assert!(!all(
            Permission::Write,
            &[Permission::Write; MAX_EDITORS + 1]
        ));
        assert!(!Writers::unknown().all_can_write());
        for (role, permission) in [
            ("admin", Permission::Admin),
            ("write", Permission::Write),
            ("read", Permission::Read),
            ("none", Permission::None),
            ("maintain", Permission::Unknown),
            ("WRITE", Permission::Unknown),
        ] {
            assert_eq!(Permission::from_role(role), permission, "{role}");
        }
        assert_eq!(
            serde_json::to_string(&Writers {
                author: Permission::Admin,
                editors: vec![Permission::Read]
            })
            .ok()
            .as_deref(),
            Some(r#"{"author":"admin","editors":["read"]}"#)
        );
    }
}
