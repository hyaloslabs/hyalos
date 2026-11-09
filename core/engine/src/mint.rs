//! Minting a manifest only within policy (S3.13, ADR-0004, ADR-0008, ADR-0034), and only tainted when
//! the step's role or inputs make it so (S3.14, ADR-0005, ADR-0047).
//!
//! The engine signs a manifest for a job only if the policy in force allows every capability the
//! manifest asks for. Each action, tool, data flow and egress host in it is put to the policy as a
//! question of its own, and one that is not allowed ends the minting. The answer is a signed manifest
//! or a refusal, never a manifest that is partly allowed.
//!
//! **Fails closed.** Nothing is allowed by default: with no operator policy the baseline alone applies,
//! and it permits nothing. A policy that errors, or that no longer validates against the schema, holds
//! the request instead of allowing it.
//!
//! **Approval.** The policy's `approved` fact is set for one question only: `merge`, in a manifest
//! whose approval names the `merge.approved` event of the job's own task and repeats exactly what it
//! approved (ADR-0012, ADR-0071). That event carries the operator's signature (ADR-0070). A merge job
//! does nothing else, and is untainted. No other irreversible action is ever minted approved.

use std::fmt;

use hyalos_policy::{Action, JobFacts, PolicyRequest, TargetFacts, codes};
use hyalos_types::capability::Field;
use hyalos_types::{
    Answer, JobId, Manifest, ManifestError, ManifestKey, ReasonCode, SignedManifest, StepInputs,
    Timestamp,
};

use crate::engine::Engine;

/// The reason codes minting gives.
pub mod mint_codes {
    use hyalos_types::ReasonCode;

    fn code(text: &str) -> ReasonCode {
        ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
    }

    /// The manifest names an action the schema does not have, or one that is asked for by another
    /// field (a tool, a data flow or a host).
    pub fn unknown_action() -> ReasonCode {
        code("mint.unknown_action")
    }

    /// The manifest declares a data flow this build does not know (ADR-0055).
    pub fn unknown_flow() -> ReasonCode {
        code("mint.unknown_flow")
    }

    /// The manifest names a tool the capability vocabulary does not accept (ADR-0085).
    pub fn unknown_tool() -> ReasonCode {
        code("mint.unknown_tool")
    }

    /// The manifest names an egress host the capability vocabulary does not accept: not a lower-case
    /// host name, or with a port or a wildcard (ADR-0085).
    pub fn unknown_host() -> ReasonCode {
        code("mint.unknown_host")
    }

    /// The manifest's job is not a running job.
    pub fn job_not_running() -> ReasonCode {
        code("mint.job_not_running")
    }

    /// The manifest's deadline is not in the future.
    pub fn expired() -> ReasonCode {
        code("mint.expired")
    }

    /// The step reads forge-authored content, or is an actor, or the manifest names a handle, and
    /// the manifest is not tainted.
    pub fn taint_required() -> ReasonCode {
        code("mint.taint_required")
    }

    /// The manifest names a handle no human released to its job's task (ADR-0063).
    pub fn handle_not_released() -> ReasonCode {
        code("mint.handle_not_released")
    }

    /// The manifest names a result that is not a well-formed output (ADR-0064).
    pub fn bad_result() -> ReasonCode {
        code("mint.bad_result")
    }

    /// The step is given plugin-authored input and the manifest holds a capability the plugin was
    /// not granted (ADR-0006, ADR-0066).
    pub fn plugin_widens() -> ReasonCode {
        code("mint.plugin_widens")
    }

    /// The manifest merges without an approval, carries an approval without merging, or its
    /// approval is not the `merge.approved` event of the job's task as that event approved it
    /// (ADR-0071).
    pub fn merge_unapproved() -> ReasonCode {
        code("mint.merge_unapproved")
    }

    /// A merge job's manifest asks for something besides the merge, or is tainted (ADR-0004,
    /// ADR-0071).
    pub fn merge_not_alone() -> ReasonCode {
        code("mint.merge_not_alone")
    }
}

/// Why no manifest was minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintRefusal {
    /// The policy says no to something the manifest asks for. `ask` names it, such as `push`,
    /// `use_tool:Bash` or `request_host:example.com`.
    Denied { ask: String, reason: ReasonCode },
    /// The policy could not answer: it errored, or the approved operator policy no longer validates.
    /// The request is held, not allowed, until the policy answers or is replaced.
    Held { ask: String, reason: ReasonCode },
    /// The manifest is not something the engine mints, whatever the policy says.
    Invalid { reason: ReasonCode },
    /// The manifest could not be signed.
    Unsignable(ManifestError),
}

impl fmt::Display for MintRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MintRefusal::Denied { ask, reason } => {
                write!(f, "policy does not allow {ask} ({reason})")
            }
            MintRefusal::Held { ask, reason } => write!(
                f,
                "policy could not answer for {ask} ({reason}); the request is held, not allowed"
            ),
            MintRefusal::Invalid { reason } => write!(f, "no manifest is minted ({reason})"),
            MintRefusal::Unsignable(e) => write!(f, "the manifest could not be signed: {e}"),
        }
    }
}

impl std::error::Error for MintRefusal {}

/// What a manifest asks for in the repo itself. The other three actions are asked for through the
/// manifest's tools, data flows and hosts, which name what they apply to.
fn is_repo_action(action: Action) -> bool {
    !matches!(
        action,
        Action::UseTool | Action::DataFlow | Action::RequestHost
    )
}

impl Engine {
    /// Signs `manifest` with `key` for a step that reads `inputs`, if its job is running, its deadline
    /// is after `now`, it is tainted whenever the step's role or inputs taint it, and the policy in
    /// force allows every capability it asks for (ADR-0004, ADR-0005, ADR-0008). Taint is derived, not
    /// requested: an actor, or a step given forge-authored content, gets only a tainted manifest. A
    /// tainted manifest for a step that does not need one is allowed, since it can do less. A step
    /// given plugin text is minted only within the plugin's grant (`StepInputs::bound`). Nothing
    /// is written to the log: no event records a manifest yet, so this is a decision and a signature.
    pub fn mint_manifest(
        &self,
        manifest: &Manifest,
        inputs: &StepInputs,
        key: &ManifestKey,
        now: Timestamp,
    ) -> Result<SignedManifest, MintRefusal> {
        let invalid = |reason: ReasonCode| MintRefusal::Invalid { reason };
        if !self.job_is_running(manifest.job) {
            return Err(invalid(mint_codes::job_not_running()));
        }
        if manifest.deadline <= now {
            return Err(invalid(mint_codes::expired()));
        }
        // A handle is raw forge text the job will read, so it taints the job like reading it would.
        if (inputs.tainted(manifest.role) || !manifest.handles.is_empty()) && !manifest.tainted {
            return Err(invalid(mint_codes::taint_required()));
        }
        // A plugin steering the step never widens it: the manifest must be within every plugin's
        // grant, and plugin-authored input with no grant allows no capability (ADR-0066).
        if !inputs.within_grants(manifest) {
            return Err(invalid(mint_codes::plugin_widens()));
        }
        // Each handle must have been released to this job's task by a human, which binds the
        // decision to the digest the manifest names (ADR-0063).
        let task = self.state().jobs().get(&manifest.job).map(|j| j.task);
        if !manifest
            .handles
            .iter()
            .all(|h| task.is_some_and(|t| self.state().handle_is_released(t, h)))
        {
            return Err(invalid(mint_codes::handle_not_released()));
        }
        // A result is what the broker reads a submission as, so it must be one a step may return.
        if manifest
            .result
            .as_ref()
            .is_some_and(|r| !hyalos_types::workflow::output_is_valid(r))
        {
            return Err(invalid(mint_codes::bad_result()));
        }
        let approved = self.merge_approval(manifest, task)?;
        let asks = asks(manifest, approved)?;
        let policies = self.policies().map_err(|_| MintRefusal::Held {
            ask: "the policy".into(),
            reason: codes::invalid(),
        })?;
        for (label, request) in asks {
            let verdict = policies.judge_request(&request);
            match verdict.answer {
                Answer::Allow => {}
                Answer::Deny => {
                    return Err(MintRefusal::Denied {
                        ask: label,
                        reason: verdict.reason,
                    });
                }
                Answer::Unknown => {
                    return Err(MintRefusal::Held {
                        ask: label,
                        reason: verdict.reason,
                    });
                }
            }
        }
        key.sign(manifest).map_err(MintRefusal::Unsignable)
    }

    /// Whether the manifest's merge is approved: `false` for a manifest that neither merges nor
    /// carries an approval, `true` only for one whose approval is exactly the job's task's
    /// `merge.approved` event, and a refusal for anything in between.
    fn merge_approval(
        &self,
        manifest: &Manifest,
        task: Option<hyalos_types::TaskId>,
    ) -> Result<bool, MintRefusal> {
        let invalid = |reason: ReasonCode| MintRefusal::Invalid { reason };
        let merging = manifest.actions.contains(Action::Merge.id());
        let approval = match (merging, &manifest.approval) {
            (false, None) => return Ok(false),
            (true, Some(approval)) => approval,
            _ => return Err(invalid(mint_codes::merge_unapproved())),
        };
        // The merge and nothing else, untainted: tainted text never steers what is merged.
        let alone = manifest.actions.len() == 1
            && manifest.tools.is_empty()
            && manifest.egress_hosts.is_empty()
            && manifest.data_flows.is_empty()
            && manifest.handles.is_empty()
            && manifest.result.is_none()
            && manifest.pr_base.is_none()
            && manifest.closes.is_none()
            && manifest.branch_prefix.is_empty();
        if !alone || manifest.tainted {
            return Err(invalid(mint_codes::merge_not_alone()));
        }
        let matches = task
            .and_then(|t| self.state().merge_of(t))
            .is_some_and(|m| {
                m.seq == approval.event
                    && m.repo == manifest.repo
                    && m.pr == approval.pr
                    && m.head == approval.head_sha
                    && m.method == approval.method
            });
        if !matches {
            return Err(invalid(mint_codes::merge_unapproved()));
        }
        Ok(true)
    }

    fn job_is_running(&self, job: JobId) -> bool {
        self.state()
            .jobs()
            .get(&job)
            .is_some_and(|j| j.is_running())
    }
}

/// Every question a manifest puts to the policy, in a fixed order: actions, tools, data flows, hosts.
/// `approved` is set for the merge question alone, and only when [`Engine::merge_approval`] found
/// the operator's approval for it.
fn asks(manifest: &Manifest, approved: bool) -> Result<Vec<(String, PolicyRequest)>, MintRefusal> {
    let request = |action: Action, name: &str| PolicyRequest {
        action,
        job: JobFacts {
            tainted: manifest.tainted,
            repo: manifest.repo.clone(),
            budget_tokens: manifest.budget.tokens,
        },
        target: TargetFacts {
            repo: manifest.repo.clone(),
            name: name.to_string(),
            // Minting asks whether the job may be given a capability on its own branches. Whether a
            // particular branch is protected is a fact of the moment of the action, not of the grant,
            // so a merge is asked as if its base were protected: the stricter answer.
            in_branch_prefix: true,
            protected: action == Action::Merge,
        },
        // See the module comment.
        approved: approved && action == Action::Merge,
        subscription_token: false,
    };
    let mut asks = Vec::new();
    for name in &manifest.actions {
        let known = Field::Actions.accepts(name);
        match Action::from_id(name).filter(|a| known && is_repo_action(*a)) {
            Some(action) => asks.push((name.clone(), request(action, ""))),
            None => {
                return Err(MintRefusal::Invalid {
                    reason: mint_codes::unknown_action(),
                });
            }
        }
    }
    for tool in &manifest.tools {
        if !Field::Tools.accepts(tool) {
            return Err(MintRefusal::Invalid {
                reason: mint_codes::unknown_tool(),
            });
        }
        asks.push((format!("use_tool:{tool}"), request(Action::UseTool, tool)));
    }
    for flow in &manifest.data_flows {
        if !Field::DataFlows.accepts(flow) {
            return Err(MintRefusal::Invalid {
                reason: mint_codes::unknown_flow(),
            });
        }
        asks.push((format!("data_flow:{flow}"), request(Action::DataFlow, flow)));
    }
    for host in &manifest.egress_hosts {
        if !Field::EgressHosts.accepts(host) {
            return Err(MintRefusal::Invalid {
                reason: mint_codes::unknown_host(),
            });
        }
        asks.push((
            format!("request_host:{host}"),
            request(Action::RequestHost, host),
        ));
    }
    Ok(asks)
}
