// The baseline (ADR-0008, ADR-0033): always loaded, and operator policy cannot remove it. A satisfied
// `forbid` overrides every `permit` whatever the order, so what is forbidden here stays forbidden
// under any operator policy. CI proves the properties in the comments with cedar-policy-symcc.

// No write outside the job's own repo.
@id("baseline.write_own_repo")
forbid (principal, action in Action::"repo_write", resource)
unless { resource.repo == principal.repo };

// No push outside the job's branch prefix.
@id("baseline.push_own_prefix")
forbid (principal, action == Action::"push", resource)
unless { resource.in_branch_prefix };

// A tainted job cannot merge.
@id("baseline.tainted_no_merge")
forbid (principal, action == Action::"merge", resource)
when { principal.tainted };

// A tainted job cannot use a subscription token.
@id("baseline.tainted_no_subscription_token")
forbid (principal, action, resource)
when { principal.tainted && context.subscription_token };

// No irreversible action without an approval.
@id("baseline.irreversible_needs_approval")
forbid (principal, action in Action::"irreversible", resource)
unless { context.approved };
