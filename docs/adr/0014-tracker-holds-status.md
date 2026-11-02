# ADR-0014: The tracker holds status; PRDs hold intent; ADRs hold decisions

**Status:** Proposed

## Context

Status lived in both the PRD YAML and the tracker, which took about 100 KB of guard scripts to keep in sync, and design notes grew into 240 KB files.

## Decision

GitHub issues are the only source of status: each epic is a parent issue with its stories as sub-issues, grouped by milestone (M1 to M4). PRDs hold intent and scope. ADRs live in `docs/adr/`, one decision each, never edited once accepted. Reference docs are generated from types, manifests and ADRs wherever possible. Trusted-core paths require human code-owner review from day one.

## Consequences

No plan-policing scripts. Agents and contributors learn why the system is shaped as it is from one folder.
