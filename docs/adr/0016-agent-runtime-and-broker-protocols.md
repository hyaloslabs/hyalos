# ADR-0016: Agent runtime, broker protocols and model credentials

**Status:** Proposed

## Context

Walking one real job end to end ("From Claude Code, I ask Hyalos to fix issue #42 in my GitHub repo and open a PR", see `docs/planning/reference-job-walk.md`) showed that no ADR says what runs inside a sandbox, how a tool with no network reaches the broker, which model protocols the broker speaks, or who holds the model credential.

Today's hivrr already ran Claude Code in worker containers with `ANTHROPIC_BASE_URL` pointed at its own proxy, a placeholder key in the container and the real key added outside. It ran readers with `--tools ""`. That path is proven, but with network access; the loopback forwarder below is new.

## Decision

**Readers and planners.** A reader needs no sandbox ([ADR-0005](0005-provenance-is-a-type.md), [ADR-0002](0002-one-binary-two-processes-sandboxes.md)). A planner that needs read access to the code runs in a sandbox with read-only tools, as an actor variant.

**Actors run any harness through one contract.** Inside the sandbox, a small forwarder listens on a loopback port and relays to the broker's channel ([ADR-0011](0011-pluggable-substrates.md)). The harness sets its base URL to that port with a placeholder key; the broker adds the real credential. v1 supports Claude Code, Codex, and the two providers' Agent SDKs. Any other runtime that honors a base-URL override works too. A harness sends tool definitions on every request, so it can only be an actor or deterministic step, never a reader.

**The broker speaks two protocols in v1:** Anthropic Messages and OpenAI Responses. The OpenAI-compatible format also covers local models (Ollama, vLLM). Both sit behind one adapter interface to stay inside the trusted-core line budget.

**Harness traffic other than model calls is off by default** (telemetry, auto-update, built-in web fetch). Anything needed goes through the egress allowlist.

**Model credentials.** v1 default: API keys held by the broker, as in [ADR-0002](0002-one-binary-two-processes-sandboxes.md). The broker never stores or injects a subscription token.

**Subscription mode (experimental flag).** The vendor's own unmodified client (Claude Code or Codex) signs in inside the sandbox with the user's own token. The broker does not handle it. It is recorded on the job event as a weaker tier, "credential inside sandbox," and is limited to jobs with no tainted input. Provider token formats are also added to the broker's content gate, but only as defense in depth: a pattern match cannot stop an agent that encodes or splits a token. The mode ships only after Anthropic and OpenAI answer whether this use is permitted.

## Consequences

The sandbox image gains a forwarder; a spike with a real harness is the M1 gate, because "no secrets in the sandbox" only means something if a real agent is working there. Two protocol adapters count against the 30,000-line budget. If the providers say subscription mode is not permitted, it is removed without touching anything else. A subscription token in the sandbox can leave through any permitted output, so it stays restricted to untainted jobs until that risk is better bounded.

## References (read November 1, 2026)

- Anthropic, Claude Code legal and compliance: [code.claude.com/docs/en/legal-and-compliance](https://code.claude.com/docs/en/legal-and-compliance). Forbids routing requests through, or collecting, storing or intermediating, Free, Pro, Max or Ultra plan credentials on behalf of users; carves out an end user signing in to the unmodified Claude Code binary with their own subscription.
- Anthropic, Claude Code authentication: [code.claude.com/docs/en/authentication](https://code.claude.com/docs/en/authentication). `claude setup-token` makes a one-year, inference-only OAuth token; revocation and spend limits are not documented.
- Anthropic, usage credits on plans: [support.claude.com/en/articles/11145838](https://support.claude.com/en/articles/11145838). Pro and Max can enable usage credits billed at API rates; no spend cap is documented on that page.
- OpenAI, API credits for ChatGPT subscribers: [developers.openai.com/api/docs/guides/chatgpt-api-credits](https://developers.openai.com/api/docs/guides/chatgpt-api-credits). Grants apply to the linked API organization and its project API keys, not to signed-in Codex usage.
- OpenAI, Codex authentication: [learn.chatgpt.com/docs/auth](https://learn.chatgpt.com/docs/auth). Part of this page was read as a model-written summary. Not read: OpenAI's `developers.openai.com/siwc/token-sharing-open-source` pages, which may address third-party use of ChatGPT-plan tokens.
