//! Cedar evaluation that cannot be turned into an allow by an error (ADR-0008, ADR-0009).
//!
//! Cedar skips a policy whose evaluation errors and reports the error only in the response's
//! diagnostics, so a `forbid` that errors, for example on an attribute the entity does not have,
//! silently stops forbidding and a `permit` beside it allows the request. [`Policies::judge`] looks
//! at the diagnostics before it looks at the decision: any error at all is `Unknown`, which holds
//! the task, and an allow is returned only when the evaluation was clean.
//!
//! Cedar's types stay inside this module. Callers hand over text (policies, entities as JSON,
//! entity ids) and get back a [`Verdict`], so a caller cannot reach the decision without the check.

use std::fmt;
use std::str::FromStr;

use cedar_policy::{
    Authorizer, Context, Decision, Entities, EntityUid, PolicyId, PolicySet, Request, Schema,
    ValidationMode, Validator,
};
use hyalos_types::{Answer, ReasonCode};

use crate::Verdict;
use crate::shipped::{BASELINE, PolicyRequest, SCHEMA};

/// A set of Cedar policies that parsed.
///
/// [`Policies::load`] is the production path: the shipped baseline plus the operator's policy,
/// validated against the shipped schema. [`Policies::parse`] is the bare evaluator, with no schema and
/// no baseline, for testing evaluation itself.
pub struct Policies {
    set: PolicySet,
    schema: Option<Schema>,
}

/// Why policy could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Not valid Cedar. The message is Cedar's.
    Parse(String),
    /// Operator policy may not use templates.
    Template,
    /// Valid Cedar that does not pass the shipped schema. A policy in this state is rejected before a
    /// human is asked to approve it (ADR-0008).
    Invalid(Vec<String>),
    /// The shipped schema or baseline did not load. A build defect; a test keeps it from shipping.
    Shipped(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Parse(e) => write!(f, "not valid Cedar policy: {e}"),
            LoadError::Template => f.write_str("operator policy may not use templates"),
            LoadError::Invalid(errors) => {
                write!(
                    f,
                    "the policy does not match the schema: {}",
                    errors.join("; ")
                )
            }
            LoadError::Shipped(e) => write!(f, "the shipped policy did not load: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Policy text that is not valid Cedar. The message is Cedar's, for the operator who wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not valid Cedar policy: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// One question for the policies: may `principal` do `action` to `resource`, given these entities.
#[derive(Debug, Clone, Copy)]
pub struct Query<'a> {
    /// Entity ids in Cedar's syntax, such as `Job::"j1"`.
    pub principal: &'a str,
    pub action: &'a str,
    pub resource: &'a str,
    /// The entities and their attributes, as Cedar's JSON array.
    pub entities: &'a str,
}

impl Policies {
    /// Cedar's parser and nothing else: no schema, no baseline. Use [`Policies::load`] for anything
    /// that decides a real request.
    pub fn parse(source: &str) -> Result<Self, ParseError> {
        PolicySet::from_str(source)
            .map(|set| Self { set, schema: None })
            .map_err(|e| ParseError(e.to_string()))
    }

    /// The shipped baseline with the operator's policy added, all validated against the shipped
    /// schema. Empty operator text gives the baseline alone. The baseline cannot be removed or
    /// changed from here: operator policies get their own ids, so one cannot take a baseline's.
    pub fn load(operator: &str) -> Result<Self, LoadError> {
        Self::load_with(BASELINE, operator)
    }

    /// [`Policies::load`] with the baseline given, so the loader's own checks on a baseline can be
    /// tested with a bad one. Nothing outside this crate can call it.
    fn load_with(baseline: &str, operator: &str) -> Result<Self, LoadError> {
        let (schema, _warnings) =
            Schema::from_cedarschema_str(SCHEMA).map_err(|e| LoadError::Shipped(e.to_string()))?;
        let shipped =
            PolicySet::from_str(baseline).map_err(|e| LoadError::Shipped(e.to_string()))?;
        let mut set = PolicySet::new();
        for policy in shipped.policies() {
            // Cedar reads `@id` as an annotation only; the policy's id is set from it here, so every
            // baseline policy has a stable name and one without a name does not load.
            let id = policy
                .annotation("id")
                .filter(|id| id.starts_with("baseline."))
                .ok_or_else(|| LoadError::Shipped("a baseline policy has no baseline id".into()))?;
            set.add(policy.new_id(PolicyId::new(id)))
                .map_err(|e| LoadError::Shipped(e.to_string()))?;
        }
        let theirs = PolicySet::from_str(operator).map_err(|e| LoadError::Parse(e.to_string()))?;
        if theirs.templates().next().is_some() {
            return Err(LoadError::Template);
        }
        for (n, policy) in theirs.policies().enumerate() {
            let renamed = policy.new_id(PolicyId::new(format!("operator.{n}")));
            set.add(renamed)
                .map_err(|e| LoadError::Parse(e.to_string()))?;
        }
        let result = Validator::new(schema.clone()).validate(&set, ValidationMode::Strict);
        if !result.validation_passed() {
            return Err(LoadError::Invalid(
                result.validation_errors().map(|e| e.to_string()).collect(),
            ));
        }
        Ok(Self {
            set,
            schema: Some(schema),
        })
    }

    /// The ids of the policies in the set: `baseline.*` first, then `operator.N`.
    pub fn ids(&self) -> Vec<String> {
        self.set.policies().map(|p| p.id().to_string()).collect()
    }

    /// Answers a typed request against the schema. Only a set made by [`Policies::load`] can: one
    /// made by `parse` has no schema to check the request against, which is unknown.
    pub fn judge_request(&self, request: &PolicyRequest) -> Verdict {
        let Some(schema) = &self.schema else {
            return unknown(codes::bad_request());
        };
        let built = (|| {
            let action = EntityUid::from_str(&request.action_uid()).ok()?;
            let context =
                Context::from_json_str(&request.context_json(), Some((schema, &action))).ok()?;
            let req = Request::new(
                EntityUid::from_str(request.principal()).ok()?,
                action,
                EntityUid::from_str(request.resource()).ok()?,
                context,
                Some(schema),
            )
            .ok()?;
            let entities = Entities::from_json_str(&request.entities_json(), Some(schema))
                .ok()?
                .add_entities(schema.action_entities().ok()?.iter().cloned(), Some(schema))
                .ok()?;
            Some((req, entities))
        })();
        match built {
            Some((req, entities)) => self.decide(&req, &entities),
            None => unknown(codes::bad_request()),
        }
    }

    /// Answers the query. Allow only if Cedar allowed it and nothing went wrong along the way.
    pub fn judge(&self, query: &Query<'_>) -> Verdict {
        let Some((request, entities)) = build(query) else {
            // A question that cannot be put to Cedar has no answer, and no answer holds the task.
            return unknown(codes::bad_request());
        };
        self.decide(&request, &entities)
    }

    fn decide(&self, request: &Request, entities: &Entities) -> Verdict {
        let response = Authorizer::new().is_authorized(request, &self.set, entities);
        // Looked at first, and whatever the decision says: an errored `forbid` is what turns a deny
        // into an allow, so the decision cannot be trusted once any policy has errored.
        if response.diagnostics().errors().next().is_some() {
            return unknown(codes::error());
        }
        match response.decision() {
            Decision::Allow => Verdict {
                answer: Answer::Allow,
                reason: codes::allowed(),
            },
            Decision::Deny => Verdict {
                answer: Answer::Deny,
                reason: codes::denied(),
            },
        }
    }
}

fn build(query: &Query<'_>) -> Option<(Request, Entities)> {
    let uid = |text: &str| EntityUid::from_str(text).ok();
    let request = Request::new(
        uid(query.principal)?,
        uid(query.action)?,
        uid(query.resource)?,
        Context::empty(),
        None,
    )
    .ok()?;
    let entities = Entities::from_json_str(query.entities, None).ok()?;
    Some((request, entities))
}

fn unknown(reason: ReasonCode) -> Verdict {
    Verdict {
        answer: Answer::Unknown,
        reason,
    }
}

/// The reason codes policy evaluation gives, recorded in the log with the answer.
pub mod codes {
    use hyalos_types::ReasonCode;

    // The strings are valid codes, which a test pins; the fallback is a code that holds the task.
    fn code(text: &str) -> ReasonCode {
        ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
    }

    /// A policy errored while being evaluated, so the decision cannot be trusted (unknown).
    pub fn error() -> ReasonCode {
        code("policy.error")
    }

    /// The question could not be put to the policies (unknown).
    pub fn bad_request() -> ReasonCode {
        code("policy.bad_request")
    }

    pub fn allowed() -> ReasonCode {
        code("policy.allowed")
    }

    pub fn denied() -> ReasonCode {
        code("policy.denied")
    }

    /// The stored operator policy no longer validates against the schema, so what depends on it is
    /// held until the operator replaces it (unknown).
    pub fn invalid() -> ReasonCode {
        code("policy.invalid")
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadError, Policies};

    const GOOD_RULE: &str =
        r#"@id("baseline.x") forbid (principal, action, resource) when { false };"#;

    #[test]
    fn a_baseline_policy_must_have_a_baseline_id() {
        for bad in [
            r#"forbid (principal, action, resource);"#,
            r#"@id("other.x") forbid (principal, action, resource);"#,
            r#"@id("operator.0") forbid (principal, action, resource);"#,
        ] {
            assert!(
                matches!(Policies::load_with(bad, ""), Err(LoadError::Shipped(_))),
                "{bad}"
            );
        }
        assert!(Policies::load_with(GOOD_RULE, "").is_ok());
    }

    #[test]
    fn a_baseline_that_does_not_match_the_schema_does_not_load() {
        let bad =
            r#"@id("baseline.x") forbid (principal, action, resource) when { principal.admin };"#;
        assert!(matches!(
            Policies::load_with(bad, ""),
            Err(LoadError::Invalid(_))
        ));
    }

    #[test]
    fn two_baseline_policies_cannot_share_an_id() {
        let twice = format!("{GOOD_RULE}\n{GOOD_RULE}");
        assert!(matches!(
            Policies::load_with(&twice, ""),
            Err(LoadError::Shipped(_))
        ));
    }
}
