//! What an installed skill or plugin may do (ADR-0005, ADR-0006, ADR-0066).
//!
//! A [`Grant`] is the capabilities a human granted a plugin. Its text ([`crate::PluginText`]) carries
//! it, so a prompt that holds plugin text also holds the grant, and the job is bounded by both: what an
//! actor steered by a plugin may do is the intersection of the plugin's grant and the job's manifest.
//! A grant can only narrow a manifest, never widen it. Until the capability vocabulary is defined
//! (S6.6), a grant is written in the manifest's own sets.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::Manifest;

/// The capabilities granted to one plugin, in the manifest's terms. A local install starts with
/// none (ADR-0006).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    #[serde(default)]
    pub actions: BTreeSet<String>,
    #[serde(default)]
    pub egress_hosts: BTreeSet<String>,
    #[serde(default)]
    pub tools: BTreeSet<String>,
    #[serde(default)]
    pub data_flows: BTreeSet<String>,
}

impl Grant {
    /// No capabilities: what a plugin has before a human grants it any.
    pub fn none() -> Self {
        Self::default()
    }

    /// `manifest` with each capability set cut to what this grant also holds. Everything else is
    /// kept, so the result is never wider than `manifest`.
    pub fn bound(&self, manifest: &Manifest) -> Manifest {
        let cut =
            |of: &BTreeSet<String>, to: &BTreeSet<String>| of.intersection(to).cloned().collect();
        Manifest {
            actions: cut(&manifest.actions, &self.actions),
            egress_hosts: cut(&manifest.egress_hosts, &self.egress_hosts),
            tools: cut(&manifest.tools, &self.tools),
            data_flows: cut(&manifest.data_flows, &self.data_flows),
            ..manifest.clone()
        }
    }

    /// Whether every capability `manifest` holds is in this grant.
    pub fn allows(&self, manifest: &Manifest) -> bool {
        manifest.actions.is_subset(&self.actions)
            && manifest.egress_hosts.is_subset(&self.egress_hosts)
            && manifest.tools.is_subset(&self.tools)
            && manifest.data_flows.is_subset(&self.data_flows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, JobId, MANIFEST_VERSION, Role, Timestamp};

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
