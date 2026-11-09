//! What an installed skill or plugin may do (ADR-0005, ADR-0006, ADR-0066, ADR-0085).
//!
//! A [`Grant`] is the capabilities a human granted a plugin. Its text ([`crate::PluginText`]) carries
//! it, so a prompt that holds plugin text also holds the grant, and the job is bounded by both: what an
//! actor steered by a plugin may do is the intersection of the plugin's grant and the job's manifest.
//! A grant can only narrow a manifest, never widen it. It is written in the capability vocabulary
//! ([`crate::capability`]), which the manifest shares.

use crate::Capabilities;

/// The capabilities granted to one plugin. A local install starts with none (ADR-0006).
pub type Grant = Capabilities;

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
