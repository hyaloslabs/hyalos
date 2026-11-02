# ADR-0014: The tracker holds status; PRDs hold intent; ADRs hold decisions

**Status:** Accepted on November 1, 2026, after review against the repository's actual practice. The clarifications from that review are folded in below.

## Context

Status lived in both the PRD YAML and the tracker, which took about 100 KB of guard scripts to keep in sync, and design notes grew into 240 KB files.

## Decision

GitHub issues are the only source of work status: each epic is a parent issue with its stories as sub-issues, grouped by milestone (M1 to M4). An ADR's own status (Proposed, Accepted, Superseded) belongs to the decision record, not to work status, and lives in the ADR and its index.

PRDs hold intent and scope. ADRs live in `docs/adr/`, one decision each, and are never edited once accepted, except that the Status line changes when a later ADR supersedes them. Planning working material lives in `docs/planning/` and is never authoritative: it is deleted, or its content folded into an ADR, once it is filed or settled. Reference docs are generated from types, manifests and ADRs wherever possible.

Trusted-core paths require human code-owner review, and `CODEOWNERS` records which paths. While the repository is private on a plan without branch protection, the platform cannot enforce this. The control until then is that a human gives an explicit instruction for each merge to `main`, and automation carries that instruction out without substituting for it. Platform enforcement is the story S10.6.

## Consequences

No plan-policing scripts. Agents and contributors learn why the system is shaped as it is from one folder. The code-owner rule is a commitment, not a mechanism, until S10.6 lands. A sole maintainer cannot approve their own pull request on GitHub, so S10.6 must also decide how that case works: a second reviewer, or a documented administrator bypass.
