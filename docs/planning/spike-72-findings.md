# Spike #72: a real harness through a loopback forwarder in a no-network sandbox

Working material, run November 1, 2026. It settled the former open question on the forwarder with a real harness. What it settled is folded into [ADR-0016](../adr/0016-agent-runtime-and-broker-protocols.md); this file keeps the evidence and the method.

## Question

Does a real harness (Claude Code, Codex, or either provider's Agent SDK) work when its only route out is a loopback port inside a sandbox with no network, relayed to the broker's channel, with a placeholder key in the sandbox? And what else must the broker and the sandbox image do for that to be safe?

## Method

- **Sandbox:** an Ubuntu 24.04 container run with `--network none --user 1000:1000 --read-only --tmpfs /tmp`. The only interface is loopback. The harness binary is mounted read-only from the host.
- **Forwarder:** `socat TCP-LISTEN:8089,bind=127.0.0.1,fork,reuseaddr UNIX-CONNECT:/run/broker.sock`, started by the image entrypoint. A Unix socket on the host is the broker's channel and is the only thing mounted in.
- **Broker stand-in:** a small Node server on that socket. In scripted mode it speaks Anthropic Messages and OpenAI Responses with no model. In passthrough mode it behaves as the real broker would: it drops the placeholder credential, adds the real key on the upstream call, removes provider-hosted tools, and never logs the key. It also acts as an allowlisting HTTP and CONNECT proxy, and can inject provider-shaped errors on demand.
- **`hyalos-submit`:** a shell script that POSTs a JSON result to the forwarder.
- **Traffic observation:** an internal Docker network with a DNS sinkhole that logs every name a harness looks up. It records names asked for, not connections by IP.
- **Models:** `claude-haiku-5-5` and `gpt-5-mini`, a few thousand tokens per run.
- **Versions:** Claude Code 2.1.316, Codex 0.114.0, Claude Agent SDK 0.3.316, OpenAI Agents SDK 0.21.0 (JavaScript), Docker 29.1.3 on an Ubuntu 24.04 host under WSL2. Everything below was observed on these versions and should be re-checked when they change.

## Results

| Check | Claude Code | Codex | Claude Agent SDK | OpenAI Agents SDK |
| --- | --- | --- | --- | --- |
| Completes a task against a live model with the base URL on the forwarder and a placeholder key | Yes, 4.2 s | Yes, about 6,600 tokens | Yes, 2.1 s | Yes, 5.3 s |
| The typed result submitted through `hyalos-submit` or an SDK tool reaches the broker | Yes | Yes | Yes | Yes |
| Placeholder reaches the broker as | `x-api-key` | `Authorization: Bearer` | `x-api-key` | `Authorization: Bearer` |
| Sends tool definitions on every request | Yes | Yes | Yes | Yes |
| Direct outbound connection from the sandbox | Fails (no route, DNS fails) | Fails | Fails | Fails |

The Claude Agent SDK ships its own native Claude Code binary, so the sandbox image does not need to provide one. Streaming works end to end: with a roughly 350-word generation, the first byte arrived in about 0.6 s and the stream ran 4.8 s (Anthropic, 406 text events) and 6.5 s (OpenAI, 992 text events), ending cleanly.

## Background traffic and the settings that stop it

| Client | Without settings | Setting | With setting |
| --- | --- | --- | --- |
| Claude Code and the Claude Agent SDK | Look up `api.anthropic.com` three times despite the base URL | `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `DISABLE_AUTOUPDATER=1`, `DISABLE_TELEMETRY=1`, `DISABLE_ERROR_REPORTING=1` | No lookups |
| Codex | Looks up `ab.chatgpt.com` 11 times and waits about 5 s at startup | `-c analytics.enabled=false` | No lookups, startup 1.1 s |
| OpenAI Agents SDK | Exports traces to `api.openai.com` (6 lookups) and starts 6.6 s slower (13.6 s against 7.0 s) | `OPENAI_AGENTS_DISABLE_TRACING=1` | No lookups |

The Claude Code settings are documented in its environment-variables reference. The Codex setting is named in the binary's own help text. `-c feedback.enabled=false` made no further difference.

## Built-in web access and provider-hosted tools

- **Claude Code** lists `WebFetch` and `WebSearch` in its default request as tools available on demand, together with `Agent`, `Workflow` and `ScheduleWakeup`. `--tools "Bash,Read,Edit"` removes all of them. `--bare` sends only `Bash`, `Edit` and `Read`.
- **Codex** advertises `web_search` as a provider-hosted tool: the provider runs it, outside the sandbox's network, and a query string can carry data out. `-c web_search=disabled` removes it. These forms did not work: `web_search=false` and `tools.web_search=false` (rejected), `--disable web_search_request` (no effect).
- **Broker-side removal works on both APIs.** The passthrough broker removed Codex's `web_search` from the upstream request, and the live call returned 200. When Claude Code's `WebSearch` tool was enabled, the harness made a separate nested request that advertised only the hosted `web_search_20250305` tool. The broker removed it, the call still returned 200, and Claude Code reported that search was unavailable instead of failing. So the removal must apply to every request, not only the first.

## Needed traffic through the egress allowlist

The sandbox sets `HTTPS_PROXY` and `HTTP_PROXY` to the forwarder and `NO_PROXY=127.0.0.1,localhost` so that model calls keep the direct channel. The broker stand-in then acts as a proxy with a host allowlist.

- **Plain client:** an allowed host succeeds over HTTPS (CONNECT) and HTTP. An unlisted host gets a refusal (403, or a failed tunnel). An IP literal is refused. A direct connection with no proxy fails.
- **Codex:** shell commands run by Codex inherit the proxy variables. A `curl` to the allowed host returned 200 and one to an unlisted host failed with `CONNECT tunnel failed, response 403`.
- **Claude Code `WebFetch`:** blocked even for an allowed host, because before each fetch it checks the domain against `api.anthropic.com`, and the allowlist correctly refuses that. Allowing `api.anthropic.com` would give a prompt-injected agent a direct route to Anthropic's API under its own account. With `--settings '{"skipWebFetchPreflight":true}'`, the allowed host returned the page and the unlisted host failed because the proxy refused it, with no connection to `api.anthropic.com`.

## How the harnesses react to errors from the broker

Errors were injected in each provider's own shape. The task was a one-line `hyalos-submit`.

| Error from the broker | Claude Code | Codex |
| --- | --- | --- |
| 429 rate limit, twice, `retry-after: 1`, then success | Retried, completed (4.8 s) | Gave up at the first error (0.6 s): `exceeded retry limit, last status: 429` |
| 529 or 503 overloaded, twice, then success | Retried, completed (529) | Retried, completed (503) |
| 402 budget exhausted, permanent | Failed at once (0.7 s) and showed the broker's message | Retried 5 times, failed after 6.9 s and showed the message |
| 403 refused by manifest, permanent | Failed at once (0.7 s) and showed the message | Retried 5 times, failed after 6.9 s and showed the message |
| 429 budget exhausted, permanent | Kept retrying (7 attempts in 45 s) until killed, no message | Gave up at once (0.5 s) with a generic message, the broker's text lost |

A 429 is the wrong code for a terminal refusal for either harness. Codex also does not retry a transient 429, so the broker must absorb upstream rate limits itself.

## Codex's own sandbox inside the container

Codex's sandbox starts inside the container in `workspace-write` and `read-only` modes, but it blocks its commands from the network, loopback included, so `hyalos-submit` could not reach the forwarder. With `-c sandbox_workspace_write.network_access=true`, the commands reach the forwarder. A write outside the workspace failed with a read-only filesystem error, but the container's own read-only root caused that, so this run does not show that Codex's sandbox confines files. The simpler setup is `--sandbox danger-full-access` with the container as the boundary.

## Other findings

- A Unix socket path is limited to about 107 characters. A long working-directory path made the broker fail to bind. Per-job socket paths in the real substrate must be short.
- Codex needs its home directory (`CODEX_HOME`) to exist before it starts.
- Claude Code requests `/v1/messages?beta=true`, so the broker's path matching must accept a query string. It selected its own model (`claude-opus-5-5`) unless told otherwise, so the broker needs a model allowlist or mapping.
- Claude Code 2.1.316 and Codex 0.114.0 are single binaries that mount cleanly into a stock Ubuntu image. Claude Code is glibc-linked and Codex is static musl. Node mounts too; the SDK tests used the host's Node 24.

## Not tested

- The Python variants of the two SDKs.
- Codex's `responses_websockets` feature, which is still under development. A TCP relay should carry it if the broker accepts the upgrade.
- Anthropic prompt caching and long-running idle connections.
- Concurrent jobs sharing one forwarder.
- macOS and other substrates. This ran on Linux with Docker.

The rig is about 350 lines and lives in [`spikes/72/`](../../spikes/72/), with a README that says how to run it. It is throwaway tooling, not product code.
