//! The shipped schema, the baseline and operator policy on top of it (S3.6, ADR-0008, ADR-0012).

use std::collections::BTreeMap;

use cedar_policy::{EntityTypeName, Schema};
use hyalos_policy::{
    Action, BASELINE, JobFacts, LoadError, Policies, PolicyRequest, SCHEMA, TargetFacts, Tier,
    codes,
};
use hyalos_types::Answer;

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

const PERMIT_ALL: &str = "permit (principal, action, resource);";

/// A request that breaks no baseline rule: a clean job acting inside its repo and prefix.
fn fine(action: Action) -> PolicyRequest {
    PolicyRequest {
        action,
        job: JobFacts {
            tainted: false,
            repo: "hyaloslabs/hyalos".into(),
        },
        target: TargetFacts {
            repo: "hyaloslabs/hyalos".into(),
            in_branch_prefix: true,
            protected: false,
        },
        approved: false,
        subscription_token: false,
    }
}

fn answer(policies: &Policies, request: &PolicyRequest) -> Answer {
    policies.judge_request(request).answer
}

fn schema() -> Res<Schema> {
    Ok(Schema::from_cedarschema_str(SCHEMA)?.0)
}

#[test]
fn the_shipped_schema_and_baseline_load_and_validate() -> Res {
    let baseline = Policies::load("")?;
    assert_eq!(
        baseline.ids(),
        [
            "baseline.write_own_repo",
            "baseline.push_own_prefix",
            "baseline.tainted_no_merge",
            "baseline.tainted_no_subscription_token",
            "baseline.irreversible_needs_approval",
        ]
    );
    assert!(BASELINE.contains("@id(\"baseline.write_own_repo\")"));
    Ok(())
}

#[test]
fn the_rust_actions_are_exactly_the_schemas_and_each_is_in_exactly_one_tier() -> Res {
    let entities = schema()?.action_entities()?;
    let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entity in entities.iter() {
        let uid = entity.uid();
        parents.insert(
            uid.id().unescaped().to_string(),
            entities
                .ancestors(&uid)
                .into_iter()
                .flatten()
                .map(|p| p.id().unescaped().to_string())
                .collect(),
        );
    }
    let tiers = [Tier::Routine, Tier::Escalated, Tier::Irreversible];
    let groups: Vec<&str> = tiers.iter().map(|t| t.group()).collect();
    // Every Rust action is in the schema, and its schema tier is the Rust one.
    for action in Action::ALL {
        let ps = parents
            .get(action.id())
            .ok_or_else(|| format!("{} is not in the schema", action.id()))?;
        let in_tiers: Vec<&&str> = groups
            .iter()
            .filter(|g| ps.iter().any(|p| p == **g))
            .collect();
        assert_eq!(
            in_tiers.len(),
            1,
            "{} is in tiers {in_tiers:?}",
            action.id()
        );
        assert_eq!(*in_tiers[0], action.tier().group(), "{}", action.id());
    }
    // And every schema action that can be asked is a Rust action: none is left out.
    let askable: Vec<&String> = parents
        .keys()
        .filter(|k| {
            !groups.contains(&k.as_str()) && !["repo_write", "branch_write"].contains(&k.as_str())
        })
        .collect();
    let rust: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
    assert_eq!(askable.len(), rust.len(), "{askable:?} against {rust:?}");
    for name in askable {
        assert!(
            rust.contains(&name.as_str()),
            "{name} is in the schema only"
        );
    }
    Ok(())
}

#[test]
fn the_irreversible_tier_holds_what_adr_0012_names() {
    for (action, tier) in [
        (Action::Merge, Tier::Irreversible),
        (Action::ChangePolicy, Tier::Irreversible),
        (Action::Publish, Tier::Irreversible),
        (Action::SpendOverCap, Tier::Irreversible),
        (Action::ExportCredentials, Tier::Irreversible),
        (Action::RequestHost, Tier::Escalated),
        (Action::Push, Tier::Routine),
        (Action::Read, Tier::Routine),
    ] {
        assert_eq!(action.tier(), tier, "{}", action.id());
    }
    assert!(Tier::Routine < Tier::Escalated && Tier::Escalated < Tier::Irreversible);
}

#[test]
fn the_baseline_alone_allows_nothing_and_a_permit_allows_what_it_does_not_forbid() -> Res {
    let baseline = Policies::load("")?;
    let open = Policies::load(PERMIT_ALL)?;
    for action in Action::ALL {
        let mut request = fine(action);
        request.approved = true;
        assert_eq!(answer(&baseline, &request), Answer::Deny, "{}", action.id());
        assert_eq!(answer(&open, &request), Answer::Allow, "{}", action.id());
        assert_eq!(baseline.judge_request(&request).reason, codes::denied());
    }
    Ok(())
}

/// Each baseline rule, with the request that breaks it and the same request mended.
#[test]
fn each_baseline_rule_forbids_what_it_says_whatever_the_operator_permits() -> Res {
    let open = Policies::load(PERMIT_ALL)?;
    type Break = fn(&mut PolicyRequest);
    let cases: Vec<(&str, Action, Break)> = vec![
        ("write outside the repo: push", Action::Push, |r| {
            r.target.repo = "elsewhere/else".into()
        }),
        ("write outside the repo: open_pr", Action::OpenPr, |r| {
            r.target.repo = "elsewhere/else".into()
        }),
        ("write outside the repo: comment", Action::Comment, |r| {
            r.target.repo = "elsewhere/else".into()
        }),
        ("write outside the repo: merge", Action::Merge, |r| {
            r.approved = true;
            r.target.repo = "elsewhere/else".into()
        }),
        ("push outside the prefix", Action::Push, |r| {
            r.target.in_branch_prefix = false
        }),
        ("tainted merge, even approved", Action::Merge, |r| {
            r.approved = true;
            r.job.tainted = true
        }),
        (
            "tainted with a subscription token: read",
            Action::Read,
            |r| {
                r.job.tainted = true;
                r.subscription_token = true
            },
        ),
        (
            "tainted with a subscription token: push",
            Action::Push,
            |r| {
                r.job.tainted = true;
                r.subscription_token = true
            },
        ),
        (
            "irreversible without approval: publish",
            Action::Publish,
            |_| {},
        ),
        (
            "irreversible without approval: change_policy",
            Action::ChangePolicy,
            |_| {},
        ),
        (
            "irreversible without approval: merge",
            Action::Merge,
            |_| {},
        ),
        (
            "irreversible without approval: spend",
            Action::SpendOverCap,
            |_| {},
        ),
        (
            "irreversible without approval: export",
            Action::ExportCredentials,
            |_| {},
        ),
    ];
    for (what, action, breaks) in cases {
        let mut request = fine(action);
        breaks(&mut request);
        assert_eq!(answer(&open, &request), Answer::Deny, "{what}");
        assert_eq!(
            open.judge_request(&request).reason,
            codes::denied(),
            "{what}"
        );
    }
    // The same requests mended are allowed, so the rules above are what denied them.
    for action in [Action::Push, Action::OpenPr, Action::Comment, Action::Read] {
        assert_eq!(
            answer(&open, &fine(action)),
            Answer::Allow,
            "{}",
            action.id()
        );
    }
    let mut approved_merge = fine(Action::Merge);
    approved_merge.approved = true;
    assert_eq!(answer(&open, &approved_merge), Answer::Allow);
    // A tainted job may still read, push inside its prefix and open a PR with the App's token.
    for action in [Action::Read, Action::Push, Action::OpenPr] {
        let mut request = fine(action);
        request.job.tainted = true;
        assert_eq!(answer(&open, &request), Answer::Allow, "{}", action.id());
    }
    // An untainted job may use a subscription token.
    let mut request = fine(Action::Read);
    request.subscription_token = true;
    assert_eq!(answer(&open, &request), Answer::Allow);
    // Approval does not widen anything but the irreversible tier.
    let mut request = fine(Action::Push);
    request.approved = true;
    request.target.in_branch_prefix = false;
    assert_eq!(answer(&open, &request), Answer::Deny);
    Ok(())
}

#[test]
fn an_operator_policy_cannot_remove_or_take_the_place_of_a_baseline_policy() -> Res {
    // A policy that names itself like a baseline policy is renamed, so both stay.
    let sneaky = r#"
        @id("baseline.irreversible_needs_approval")
        permit (principal, action, resource);
        @id("baseline.write_own_repo")
        permit (principal, action == Action::"push", resource);
    "#;
    let policies = Policies::load(sneaky)?;
    let ids = policies.ids();
    assert_eq!(ids.len(), 7);
    for id in [
        "baseline.irreversible_needs_approval",
        "baseline.write_own_repo",
    ] {
        assert_eq!(ids.iter().filter(|i| *i == id).count(), 1, "{id}");
    }
    assert_eq!(ids[5..], ["operator.0", "operator.1"]);
    assert_eq!(answer(&policies, &fine(Action::Publish)), Answer::Deny);
    let mut elsewhere = fine(Action::Push);
    elsewhere.target.repo = "elsewhere/else".into();
    assert_eq!(answer(&policies, &elsewhere), Answer::Deny);
    Ok(())
}

#[test]
fn an_operator_can_forbid_more_and_cannot_move_an_action_to_a_lower_tier() -> Res {
    let stricter = Policies::load(
        r#"permit (principal, action, resource);
           forbid (principal, action == Action::"push", resource) when { resource.protected };"#,
    )?;
    let mut request = fine(Action::Push);
    assert_eq!(answer(&stricter, &request), Answer::Allow);
    request.target.protected = true;
    assert_eq!(answer(&stricter, &request), Answer::Deny);

    // Permits written for the routine tier do not reach an irreversible action, and a permit for the
    // irreversible one still needs the approval.
    let routine_only =
        Policies::load(r#"permit (principal, action in Action::"routine", resource);"#)?;
    let mut merge = fine(Action::Merge);
    merge.approved = true;
    assert_eq!(answer(&routine_only, &merge), Answer::Deny);
    let irreversible =
        Policies::load(r#"permit (principal, action in Action::"irreversible", resource);"#)?;
    assert_eq!(answer(&irreversible, &fine(Action::Merge)), Answer::Deny);
    assert_eq!(answer(&irreversible, &merge), Answer::Allow);
    Ok(())
}

#[test]
fn operator_policy_that_does_not_match_the_schema_is_rejected_with_the_reason() -> Res {
    let bad = [
        (
            "an attribute that is not there",
            r#"permit (principal, action, resource) when { principal.admin };"#,
        ),
        (
            "an action that is not there",
            r#"permit (principal, action == Action::"nuke", resource);"#,
        ),
        (
            "the wrong type",
            r#"permit (principal, action, resource) when { principal.tainted == "yes" };"#,
        ),
        (
            "a principal type that is not there",
            r#"permit (principal == User::"u", action, resource);"#,
        ),
        (
            "a context field that is not there",
            r#"permit (principal, action, resource) when { context.root };"#,
        ),
    ];
    for (what, source) in bad {
        match Policies::load(source) {
            Err(LoadError::Invalid(errors)) => assert!(!errors.is_empty(), "{what}"),
            Err(other) => return Err(format!("{what}: {other}").into()),
            Ok(_) => return Err(format!("{what}: was accepted").into()),
        }
    }
    assert!(matches!(
        Policies::load("permit (principal"),
        Err(LoadError::Parse(_))
    ));
    assert!(matches!(
        Policies::load("permit (principal == ?principal, action, resource);"),
        Err(LoadError::Template)
    ));
    Ok(())
}

#[test]
fn text_in_a_repo_name_cannot_change_the_question() -> Res {
    let open = Policies::load(PERMIT_ALL)?;
    // If this were pasted into JSON unescaped, it would end the string and make the job untainted
    // and the repos equal.
    let hostile = r#"x","tainted":false,"repo":"y"#;
    for (job_repo, target_repo, expected) in [
        (hostile, hostile, Answer::Allow),
        (hostile, "y", Answer::Deny),
        ("y", hostile, Answer::Deny),
        ("a\\b", "a\\b", Answer::Allow),
        ("a\nb", "a\nb", Answer::Allow),
        ("a\nb", "a b", Answer::Deny),
        ("", "", Answer::Allow),
    ] {
        let mut request = fine(Action::Push);
        request.job.repo = job_repo.into();
        request.target.repo = target_repo.into();
        assert_eq!(
            answer(&open, &request),
            expected,
            "{job_repo:?} against {target_repo:?}"
        );
    }
    // And a tainted job stays tainted whatever its repo says.
    let mut request = fine(Action::Merge);
    request.approved = true;
    request.job.tainted = true;
    request.job.repo = hostile.into();
    request.target.repo = hostile.into();
    assert_eq!(answer(&open, &request), Answer::Deny);
    Ok(())
}

#[test]
fn a_set_without_the_schema_cannot_answer_a_typed_request() -> Res {
    let bare = Policies::parse(PERMIT_ALL)?;
    let verdict = bare.judge_request(&fine(Action::Read));
    assert_eq!(verdict.answer, Answer::Unknown);
    assert_eq!(verdict.reason, codes::bad_request());
    Ok(())
}

#[test]
fn the_schema_names_the_entity_types_the_engine_builds() -> Res {
    let names: Vec<String> = schema()?
        .entity_types()
        .map(|t: &EntityTypeName| t.to_string())
        .collect();
    assert!(
        names.contains(&"Job".to_string()) && names.contains(&"Target".to_string()),
        "{names:?}"
    );
    Ok(())
}
