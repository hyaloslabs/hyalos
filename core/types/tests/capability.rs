//! Pins the capability vocabulary (ADR-0085). A failure here means a field was added, removed or
//! renamed, or a field accepts other names. That is a change to what a job or a plugin may be given:
//! it needs an ADR, and it bumps `CAPABILITY_VERSION`.

use std::collections::BTreeSet;

use hyalos_types::capability::{Field, action};
use hyalos_types::{
    Budget, CAPABILITY_VERSION, Capabilities, JobId, MANIFEST_VERSION, Manifest, Role, Timestamp,
    flow,
};

const NEEDS_AN_ADR: &str =
    "the capability vocabulary changed: write an ADR and bump CAPABILITY_VERSION (ADR-0085)";

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.map(String::from).into()
}

#[test]
fn the_vocabulary_is_version_1_with_four_fields() {
    assert_eq!(CAPABILITY_VERSION, 1, "{NEEDS_AN_ADR}");
    assert_eq!(
        Field::ALL.map(Field::name),
        ["actions", "egress_hosts", "tools", "data_flows"],
        "{NEEDS_AN_ADR}"
    );
    assert_eq!(
        Field::ALL.map(Field::asked_as),
        [
            None,
            Some("request_host"),
            Some("use_tool"),
            Some("data_flow")
        ],
        "{NEEDS_AN_ADR}"
    );
    assert_eq!(
        action::ALL,
        [
            "read",
            "push",
            "open_pr",
            "comment",
            "merge",
            "change_policy",
            "publish",
            "spend_over_cap",
            "export_credentials"
        ],
        "{NEEDS_AN_ADR}"
    );
    assert_eq!(
        flow::ALL,
        ["job->model", "job->forge", "job->web"],
        "{NEEDS_AN_ADR}"
    );
    // The struct writes exactly those fields, in that order.
    let every = Capabilities {
        actions: set(["read"]),
        egress_hosts: set(["docs.rs"]),
        tools: set(["search"]),
        data_flows: set(["job->web"]),
    };
    assert_eq!(
        serde_json::to_string(&every).unwrap(),
        r#"{"actions":["read"],"egress_hosts":["docs.rs"],"tools":["search"],"data_flows":["job->web"]}"#,
        "{NEEDS_AN_ADR}"
    );
    assert!(every.check().is_ok());
}

/// The manifest holds every field of the vocabulary under the same name, so a plugin's capabilities
/// and a job's are the same sets.
#[test]
fn the_manifest_holds_every_field_of_the_vocabulary() {
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        job: JobId::from_seq(2),
        role: Role::Actor,
        tainted: true,
        repo: "octo/repo".into(),
        branch_prefix: "hyalos/job-2".into(),
        pr_base: None,
        actions: set(["read", "push"]),
        egress_hosts: set(["docs.rs"]),
        tools: set(["search"]),
        data_flows: set(["job->model"]),
        budget: Budget { tokens: 100 },
        deadline: Timestamp::from_millis(9),
        approval: None,
        handles: BTreeSet::new(),
        result: None,
        closes: None,
    };
    let written = serde_json::to_value(&manifest).unwrap();
    let capabilities = manifest.capabilities();
    for field in Field::ALL {
        let names: BTreeSet<String> =
            serde_json::from_value(written[field.name()].clone()).unwrap();
        assert_eq!(&names, capabilities.field(field), "{}", field.name());
    }
    assert!(capabilities.allows(&manifest));
    assert_eq!(capabilities.bound(&manifest), manifest);
}
