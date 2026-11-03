//! A policy error holds the task and never allows (S3.15, ADR-0008, ADR-0009).

use hyalos_policy::{Evaluate, Fault, GateRequest, Outcome, Policies, Query, Verdict, ask, codes};
use hyalos_sim::Sim;
use hyalos_types::{Answer, Clock, Digest, Gate, HoldReason, Stakes, Timestamp};

/// An entity with no attributes: anything that reads one of them errors.
const BARE: &str = r#"[{"uid":{"type":"Resource","id":"r1"},"attrs":{},"parents":[]}]"#;
const MARKED: &str =
    r#"[{"uid":{"type":"Resource","id":"r1"},"attrs":{"sensitive":true},"parents":[]}]"#;
const UNMARKED: &str =
    r#"[{"uid":{"type":"Resource","id":"r1"},"attrs":{"sensitive":false},"parents":[]}]"#;

fn query(entities: &str) -> Query<'_> {
    Query {
        principal: r#"Job::"j1""#,
        action: r#"Action::"push""#,
        resource: r#"Resource::"r1""#,
        entities,
    }
}

/// The shape ADR-0008 warns about: a baseline `forbid` that reads an attribute, and an operator
/// `permit` beside it.
const BASELINE_FORBID_THAT_CAN_ERROR: &str = "
    forbid (principal, action, resource) when { resource.sensitive };
    permit (principal, action, resource);
";

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn parse(source: &str) -> Res<Policies> {
    Ok(Policies::parse(source)?)
}

/// What Cedar says on its own, to show the tests below would fail if the check were not there.
fn raw_cedar_decision(source: &str, entities: &str) -> Res<(cedar_policy::Decision, usize)> {
    use cedar_policy::{Authorizer, Context, Entities, EntityUid, PolicySet, Request};
    use std::str::FromStr;
    let set = PolicySet::from_str(source)?;
    let q = query(entities);
    let uid = |t: &str| -> Res<EntityUid> { Ok(EntityUid::from_str(t)?) };
    let request = Request::new(
        uid(q.principal)?,
        uid(q.action)?,
        uid(q.resource)?,
        Context::empty(),
        None,
    )?;
    let entities = Entities::from_json_str(entities, None)?;
    let response = Authorizer::new().is_authorized(&request, &set, &entities);
    Ok((response.decision(), response.diagnostics().errors().count()))
}

#[test]
fn cedar_alone_allows_when_a_forbid_errors_which_is_the_hazard() -> Res {
    let (decision, errors) = raw_cedar_decision(BASELINE_FORBID_THAT_CAN_ERROR, BARE)?;
    assert_eq!(decision, cedar_policy::Decision::Allow);
    assert_eq!(errors, 1, "the error is reported only in the diagnostics");
    Ok(())
}

#[test]
fn a_baseline_forbid_that_errors_on_a_missing_attribute_holds_instead_of_allowing() -> Res {
    let verdict = parse(BASELINE_FORBID_THAT_CAN_ERROR)?.judge(&query(BARE));
    assert_eq!(
        verdict,
        Verdict {
            answer: Answer::Unknown,
            reason: codes::error()
        }
    );
    Ok(())
}

#[test]
fn a_gate_built_on_it_holds_the_task_as_unknown() -> Res {
    struct Cedar<'a>(&'a Policies, &'a str);
    impl Evaluate for Cedar<'_> {
        fn evaluate(&mut self, _: &GateRequest) -> Result<Verdict, Fault> {
            Ok(self.0.judge(&query(self.1)))
        }
    }
    let policies = parse(BASELINE_FORBID_THAT_CAN_ERROR)?;
    let sim = Sim::new(1);
    let clock = sim.clock();
    let request = GateRequest {
        gate: Gate::Egress,
        inputs: Digest::from_bytes([9; 32]),
        stakes: Stakes::routine(),
    };
    let deadline = Timestamp::from_millis(clock.now().as_millis() + 1_000);
    let outcome = ask(&mut Cedar(&policies, BARE), &clock, deadline, &request);
    assert_eq!(
        outcome,
        Outcome::Hold {
            reason: codes::error(),
            why: HoldReason::Unknown
        }
    );
    Ok(())
}

#[test]
fn a_clean_evaluation_is_allowed_or_denied_as_cedar_decides() -> Res {
    let policies = parse(BASELINE_FORBID_THAT_CAN_ERROR)?;
    // The forbid reads the attribute fine, and does not apply.
    let allowed = policies.judge(&query(UNMARKED));
    assert_eq!(
        (allowed.answer, allowed.reason),
        (Answer::Allow, codes::allowed())
    );
    // The forbid applies and overrides the permit.
    let denied = policies.judge(&query(MARKED));
    assert_eq!(
        (denied.answer, denied.reason),
        (Answer::Deny, codes::denied())
    );
    // No policy applies at all: Cedar's default is deny.
    let none = parse(r#"permit (principal, action == Action::"read", resource);"#)?;
    assert_eq!(none.judge(&query(BARE)).answer, Answer::Deny);
    assert_eq!(parse("")?.judge(&query(BARE)).answer, Answer::Deny);
    Ok(())
}

#[test]
fn any_error_is_unknown_even_when_the_decision_would_be_deny() -> Res {
    // A satisfied forbid says deny, and a second policy errors: the decision is no longer one to
    // act on, and ADR-0008 says any evaluation error is unknown.
    let source = "
        forbid (principal, action, resource);
        permit (principal, action, resource) when { resource.sensitive };
    ";
    assert_eq!(
        raw_cedar_decision(source, BARE)?.0,
        cedar_policy::Decision::Deny
    );
    assert_eq!(parse(source)?.judge(&query(BARE)).answer, Answer::Unknown);
    Ok(())
}

#[test]
fn an_error_in_a_permit_alone_is_unknown_not_deny() -> Res {
    let source = "permit (principal, action, resource) when { resource.sensitive };";
    let verdict = parse(source)?.judge(&query(BARE));
    assert_eq!(
        (verdict.answer, verdict.reason),
        (Answer::Unknown, codes::error())
    );
    Ok(())
}

#[test]
fn arithmetic_overflow_and_a_failed_extension_call_are_errors_too() {
    let mut evaluated = 0;
    for body in [
        "9223372036854775807 + 1 > 0",
        r#"ip("not an address").isIpv4()"#,
    ] {
        // Some of these may be rejected when the policy is parsed; those never get to be evaluated.
        let Ok(policies) = Policies::parse(&format!(
            "forbid (principal, action, resource) when {{ {body} }}; permit (principal, action, resource);"
        )) else {
            continue;
        };
        evaluated += 1;
        assert_eq!(
            policies.judge(&query(BARE)).answer,
            Answer::Unknown,
            "{body}"
        );
    }
    assert!(evaluated >= 1, "at least one of them must reach evaluation");
}

#[test]
fn a_question_that_cannot_be_put_to_cedar_holds() -> Res {
    let policies = parse("permit (principal, action, resource);")?;
    let good = query(BARE);
    for bad in [
        Query {
            principal: "not an entity",
            ..good
        },
        Query { action: "", ..good },
        Query {
            resource: "Resource::",
            ..good
        },
        Query {
            entities: "{ not json",
            ..good
        },
        Query {
            entities: r#"[{"uid":1}]"#,
            ..good
        },
    ] {
        let verdict = policies.judge(&bad);
        assert_eq!(
            (verdict.answer, verdict.reason),
            (Answer::Unknown, codes::bad_request()),
            "{bad:?}"
        );
    }
    // And the same question, put properly, is allowed.
    assert_eq!(policies.judge(&good).answer, Answer::Allow);
    Ok(())
}

#[test]
fn policy_text_that_is_not_cedar_is_refused_when_parsed() {
    for bad in ["permit (", "forbid (principal, action) ;", "not a policy"] {
        let refused = Policies::parse(bad).is_err();
        assert!(refused, "{bad}");
        if let Err(err) = Policies::parse(bad) {
            assert!(
                err.to_string().starts_with("not valid Cedar policy"),
                "{err}"
            );
        }
    }
}

#[test]
fn the_reason_codes_are_the_valid_codes_the_log_records() {
    assert_eq!(codes::error().as_str(), "policy.error");
    assert_eq!(codes::bad_request().as_str(), "policy.bad_request");
    assert_eq!(codes::allowed().as_str(), "policy.allowed");
    assert_eq!(codes::denied().as_str(), "policy.denied");
}
