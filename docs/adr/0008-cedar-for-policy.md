# ADR-0008: Cedar for policy

**Status:** Proposed

## Context

Manifests, install policy and approval tiers are all policy. Hand-written checks are hard to reason about as a whole.

## Decision

Express manifest constraints, install policy and approval tiers in Cedar, evaluated by the engine and broker. Use Cedar's automated analysis in CI to prove properties such as "no policy lets a tainted job merge to a protected branch."

## Consequences

Policies are data people can read and diff. Some checks (byte-level content gates) stay in code. Cedar becomes a dependency of the trusted core.
