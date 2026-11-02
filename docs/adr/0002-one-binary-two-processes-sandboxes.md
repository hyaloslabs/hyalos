# ADR-0002: One binary, two processes, sandboxes

**Status:** Proposed. The original open question on provider keys in the broker was settled in the November 1, 2026 planning session; folded in below.

## Context

About 15 services were the install barrier, and most of their internal identity machinery existed only because they talked over a network. The only boundaries that carry security are the sandbox, the component that mediates the sandbox's effects, and the component holding long-lived authority.

## Decision

One `hyalos` binary runs as the **engine** (trusted; reachable only by the operator and driving agent; holds long-lived secrets, mints manifests, controls substrates) and launches itself as the **broker** (the sandboxes' only route out; holds per-job credentials and provider keys). Nothing a sandbox can reach holds substrate control or the manifest-signing key.

**Provider credentials.** The broker holds the provider API keys and makes every model call; model calls do not route through the engine. LLM provider keys cannot be scoped per job, so v1 bounds the loss instead: a dedicated key per install, a provider-side spend cap, and per-job metering against the manifest budget. v1 uses API keys. The broker never stores or injects a subscription token (see [ADR-0016](0016-agent-runtime-and-broker-protocols.md) for the experimental subscription mode).

**Untrusted input is parsed in the broker.** Forge reads, reader model calls and schema validation of model output all happen in the broker, which already holds the forge token and makes the model call. Three conditions keep this from making the broker the soft spot:

- strict parsing: a size cap, a depth cap, and rejection of unknown fields;
- a validation failure fails the job, never the broker process;
- the engine receives only typed values over its own trusted channel, never model text or raw forge content.

## Consequences

No internal service mesh, per-service keys or internal networks. A broker compromise yields live jobs' scoped credentials and provider keys, not the ability to mint jobs; a capped, dedicated provider key bounds what that costs. Modules inside the engine share a process, so untrusted parsing belongs in sandboxes or the broker, and the broker's parsing surface must stay small and fuzzed ([ADR-0013](0013-deterministic-simulation-and-evidence-gated-releases.md)).
