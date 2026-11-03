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

use cedar_policy::{Authorizer, Context, Decision, Entities, EntityUid, PolicySet, Request};
use hyalos_types::{Answer, ReasonCode};

use crate::Verdict;

/// A set of Cedar policies that parsed.
pub struct Policies(PolicySet);

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
    pub fn parse(source: &str) -> Result<Self, ParseError> {
        PolicySet::from_str(source)
            .map(Self)
            .map_err(|e| ParseError(e.to_string()))
    }

    /// Answers the query. Allow only if Cedar allowed it and nothing went wrong along the way.
    pub fn judge(&self, query: &Query<'_>) -> Verdict {
        let Some((request, entities)) = build(query) else {
            // A question that cannot be put to Cedar has no answer, and no answer holds the task.
            return unknown(codes::bad_request());
        };
        let response = Authorizer::new().is_authorized(&request, &self.0, &entities);
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
}
