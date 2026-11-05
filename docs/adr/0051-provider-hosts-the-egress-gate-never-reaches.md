# ADR-0051: Provider hosts the egress gate never reaches

**Status:** Proposed

## Context

[ADR-0016](0016-agent-runtime-and-broker-protocols.md) says a prompt-injected harness could use a model provider's API with an attacker's own key. A request that carries the attacker's key would bypass the broker's budget and send the job's context to an account the operator does not control. So the egress gate ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md)) refuses `api.anthropic.com` and `api.openai.com` even when a manifest lists them. Model calls go through the relay, with the broker's key.

That rule matches only those two exact names. S3.5c (#173) asks about the provider endpoints it misses:
- **Names under the API hosts.** A regional endpoint under `api.openai.com` is the same API under another name.
- **The operator's own bases.** `provider.keys` can point the broker at another host with `anthropic.base` or `openai.base` ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md)), such as a corporate gateway. A harness that reached that host directly might use it without the broker's key, or with a key of its own, or find the gateway trusts the network it comes from.

A manifest lists its egress hosts within the operator's policy, so reaching any of these needs a listed host. The refusal is the backstop for a listing made by mistake or a policy that allows more than it meant to.

## Decision

**The egress gate never reaches a provider host, whatever the manifest lists.** A provider host is any of these, or any name under one:
- `api.anthropic.com` and `api.openai.com` (`egress::NEVER`, as before);
- the host of each provider base the broker was started with, read from `provider.keys` (`anthropic.base`, `openai.base`, or the default when one is left out).

"Under" means at a label boundary. `eu.api.openai.com` and `x.llm.corp.example` are refused, but `api.openai.com.evil.example`, `notapi.openai.com` and `xllm.corp.example` are not provider hosts. Names are compared as the gate already compares them: lowercase, without a trailing dot.

**Only the API hosts and the names under them are refused, not the provider's whole domain.** `anthropic.com` and `docs.anthropic.com` are not refused. A job may need a provider's documentation, and the operator may list it. Refusing the registrable domain would block that, and still not finish the list: providers also serve their models through cloud platforms under other names. No list kept here can name every endpoint that takes a key. The manifest's allowlist, within policy, is the control; this rule covers the names that are certainly a provider's API.

**A base whose host is an address adds nothing.** The gate refuses every IP literal already (`egress.ip_literal`).

**The refusal has its own reason, `egress.provider_host`.** It is still a 403 to the harness and the egress gate's `deny` to the engine, with the SHA-256 of `host:port` as before. A provider host listed by the manifest used to be refused as `egress.host_not_allowed`, which read as if the manifest had not listed it. The new reason tells the operator reading the log that the manifest listed a provider host. The check comes before the manifest's list, so a provider host is `egress.provider_host` whether or not it was listed.

**The broker builds the list once, when it starts.** `Egress::refusing_bases(providers.bases())` in `hyalos broker`, beside the relays built from the same file. Nothing in configuration can remove a host from the list, only add one by naming a base. Tests reach it from code like the rest of `Egress` (`Egress::with_resolver` keeps `NEVER`).

## Consequences

Neither the operator's gateway nor a regional endpoint is reachable through the proxy by listing it. The tests:
- `core/broker/src/egress.rs`: reading a base's host (port, path, trailing dot, case, an address), and the label-boundary match, including names beside a provider host that are not under it.
- `core/broker/tests/egress.rs`: a manifest that lists a configured base's host, a name under it and `eu.api.openai.com`, all of which resolve to a public address in the test, and gets `egress.provider_host` for each, over CONNECT and plain HTTP. A listed `docs.anthropic.com` is still reached.
- `core/hyalos/tests/run_job.rs`: through `hyalos run` and the real `hyalos broker`, with `openai.base = http://localhost:9` in `provider.keys` and `localhost` listed by the manifest. The refusal is recorded as `gate.answered` with `egress.provider_host`. This shows the broker command passes the bases to the gate.

The engine still signs a manifest that lists a provider host, if policy allows the host; the broker refuses it at run time. Refusing it in `mint_manifest` as well would need the engine to know the provider bases, which only the broker reads. That is not built.

Not here:
- **The forge's hosts.** A listed `api.github.com` is reachable through the proxy. A harness has no GitHub token, but one could bring its own. Whether to refuse the forge's API and git hosts as well belongs with the forge's other egress questions.
- **Providers on cloud platforms** (Bedrock, Vertex AI, Azure OpenAI) when Hyalos does not call them. If it calls them one day, their bases are refused by this rule.
