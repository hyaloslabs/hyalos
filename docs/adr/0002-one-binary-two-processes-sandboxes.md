# ADR-0002: One binary, two processes, sandboxes

**Status:** Proposed. Depends on [open question 1](../planning/open-questions.md) (provider keys in the broker).

## Context

About 15 services were the install barrier, and most of their internal identity machinery existed only because they talked over a network. The only boundaries that carry security are the sandbox, the component that mediates the sandbox's effects, and the component holding long-lived authority.

## Decision

One `hyalos` binary runs as the **engine** (trusted; reachable only by the operator and driving agent; holds long-lived secrets, mints manifests, controls substrates) and launches itself as the **broker** (the sandboxes' only route out; holds per-job credentials and provider keys). Nothing a sandbox can reach holds substrate control or the manifest-signing key.

## Consequences

No internal service mesh, per-service keys or internal networks. A broker compromise yields live jobs' scoped credentials and provider keys, not the ability to mint jobs. Modules inside the engine share a process, so untrusted parsing belongs in sandboxes or the broker.
